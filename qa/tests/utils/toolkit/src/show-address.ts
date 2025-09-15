// Wrapper that runs `midnight-node-toolkit show-address` inside the official image
// and returns the first "mn..." address printed by the tool.

import { GenericContainer } from 'testcontainers';
import { getNetworkId, type ChainId } from './env-registry.ts';

export type AddressType = 'shielded' | 'unshielded';

export interface ShowAddressParams {
  chain: ChainId;
  addressType: AddressType;
  seed: string; // 64-hex seed
  image?: string; // override toolkit image if needed
}

export async function showAddress({
  chain,
  addressType,
  seed,
  image = 'ghcr.io/midnight-ntwrk/midnight-node-toolkit:latest',
}: ShowAddressParams): Promise<string> {
  const networkId = getNetworkId(chain);

  // ENTRYPOINT of the image is the binary, so we pass subcommand + args only
  const cmd = ['show-address', '--network', networkId, `--${addressType}`, '--seed', seed];

  let output = '';

  // Promise that resolves the first time an address is spotted in the logs
  let resolveFound!: () => void;
  let rejectFound!: (e: Error) => void;
  const found = new Promise<void>((resolve, reject) => {
    resolveFound = resolve;
    rejectFound = reject;
  });

  // Simple non-global regex (avoid lastIndex quirks)
  const addrRegex = /mn\S+/;

  // Safety timeout in case the tool never prints an address
  const timeout = setTimeout(
    () =>
      rejectFound(
        new Error(
          `Timed out waiting for address.\n--- output so far ---\n${output}\n----------------------`,
        ),
      ),
    20_000,
  );

  const container = await new GenericContainer(image)
    .withCommand(cmd)
    .withStartupTimeout(60_000)
    .withLogConsumer((stream) => {
      const onChunk = (b: Buffer) => {
        const s = b.toString('utf8');
        output += s;
        if (addrRegex.test(output)) {
          clearTimeout(timeout);
          resolveFound();
        }
      };
      stream.on('data', onChunk);
      stream.on('err', onChunk);
    })
    .start();

  try {
    // Wait until we see an address (or the timeout triggers)
    await found;

    // Give the log stream a tiny window to flush, then stop
    await new Promise((r) => setTimeout(r, 200));
    await container.stop();

    // Parse the first "mn..." token from the aggregated output
    const text = output.trim();
    const m = text.match(/mn\S+/);
    if (!m) {
      throw new Error(
        `Could not parse address from toolkit output.\n--- output ---\n${output}\n--------------`,
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
