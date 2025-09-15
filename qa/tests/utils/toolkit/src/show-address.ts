import { GenericContainer } from "testcontainers";
import { ENV, type ChainId } from "./env-registry.ts";

export type AddressType = "shielded" | "unshielded";

export interface ShowAddressParams {
  chain: ChainId;
  addressType: AddressType;
  seed: string; // 64-hex
  image?: string; // override if needed
}

/**
 * Runs `midnight-node-toolkit show-address` inside the toolkit image.
 * Attaches a log consumer before start, resolves as soon as an "mn..." token appears,
 * then stops the container and returns the parsed address.
 */
export async function showAddress({
  chain,
  addressType,
  seed,
  image = "ghcr.io/midnight-ntwrk/midnight-node-toolkit:latest",
}: ShowAddressParams): Promise<string> {
  const { networkId } = ENV[chain];

  // The toolkit image has ENTRYPOINT set to the binary, so we pass subcommand + args only
  const cmd = [
    "show-address",
    "--network",
    networkId,
    `--${addressType}`,
    "--seed",
    seed,
  ];

  let output = "";

  let resolveFound!: () => void;
  let rejectFound!: (e: Error) => void;
  const found = new Promise<void>((resolve, reject) => {
    resolveFound = resolve;
    rejectFound = reject;
  });

  // Simple (non-global) regex avoids lastIndex quirks
  const addrRegex = /mn\S+/;

  // Safety timeout in case the tool doesn't print anything
  const timeout = setTimeout(
    () =>
      rejectFound(
        new Error(
          `Timed out waiting for address.\n--- output so far ---\n${output}\n----------------------`
        )
      ),
    20_000
  );

  const container = await new GenericContainer(image)
    .withCommand(cmd)
    .withStartupTimeout(60_000)
    .withLogConsumer((stream) => {
      const onChunk = (b: Buffer) => {
        const s = b.toString("utf8");
        output += s;
        if (addrRegex.test(output)) {
          clearTimeout(timeout);
          resolveFound();
        }
      };
      stream.on("data", onChunk);
      stream.on("err", onChunk);
    })
    .start();

  try {
    // Wait until we see an address (or timeout fires)
    await found;

    // Small flush window, then stop
    await new Promise((r) => setTimeout(r, 200));
    await container.stop();

    // Parse the first "mn..." token anywhere in the output
    const text = output.trim();
    const m = text.match(/mn\S+/);
    if (!m) {
      throw new Error(
        `Could not parse address from toolkit output.\n--- output ---\n${output}\n--------------`
      );
    }
    return m[0];
  } catch (e) {
    // Ensure cleanup on failure
    try {
      await container.stop();
    } catch {}
    throw e;
  }
}
