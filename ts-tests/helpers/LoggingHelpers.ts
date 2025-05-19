import { type Readable } from 'stream';
import { type StartedDockerComposeEnvironment } from 'testcontainers';
import { Commons } from '../utils/Commons';

export function verifyThatLogIsPresent(stream: Readable, regexToWaitFor: RegExp, timeoutMs: number): Promise<void> {
  return new Promise((resolve, reject) => {
    let textAppeared = false;
    const timeoutId = setTimeout(() => {
      stream.destroy();
      if (!textAppeared) {
        reject(new Error(`Timeout: Log entry not found: ${regexToWaitFor}`));
      }
    }, timeoutMs);
    stream
      .on('data', (line) => {
        console.log(line)
        if (regexToWaitFor.test(line as string)) {
          textAppeared = true;
          clearTimeout(timeoutId);
          resolve();
        }
      })
      .on('error', (error) => {
        clearTimeout(timeoutId);
        console.error(error);
      })
      .on('end', () => {
        if (!textAppeared) {
          clearTimeout(timeoutId);
          reject(new Error(`Stream ended: Log entry not found: ${regexToWaitFor}`));
        }
      });
  });
}

export async function waitForLogAndVerifyIfLogPresent(
  composeEnvironment: StartedDockerComposeEnvironment,
  containerName: string,
  regexToWaitFor: RegExp,
  maxRetries: number,
  retryDelayMs: number,
) {
  let attempt = 0;

  while (attempt < maxRetries) {
    try {
      await verifyThatLogIsPresent(await composeEnvironment.getContainer(containerName).logs(), regexToWaitFor, 5000);
      console.log('Log verification succeeded.');
      break;
    } catch (error) {
      console.log(`Attempt ${attempt + 1} failed: ${error.message}`);
      attempt++;
      if (attempt < maxRetries) {
        console.log(`Retrying in ${retryDelayMs}ms...`);
        await Commons.sleep(retryDelayMs);
      } else {
        console.error('Max retries reached, failing...');
        throw error;
      }
    }
  }
}
