import fs from 'fs';
import path from 'path';
import { type StartedDockerComposeEnvironment } from 'testcontainers';

// eslint-disable-next-line @typescript-eslint/no-extraneous-class
export class Commons {
  public static currentDir = path.resolve(new URL(import.meta.url).pathname, '..');

  public static sleep(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
  }

  public static getJsonFromFile(file: string): string {
    return JSON.stringify(JSON.parse(Commons.getTxTemplate(file)));
  }

  public static getTxTemplate(file: string): string {
    const filePath = path.join(__dirname, '../environment/midnight-node/tx_mn_format', file);
    return fs.readFileSync(filePath, 'utf8');
  }

  public static isFileEmpty(filePath: string): boolean {
    try {
      const fileContent = fs.readFileSync(filePath, 'utf8');
      return fileContent.length === 0;
    } catch (error) {
      console.error('Error reading the file:', error);
      return false;
    }
  }

  public static async restartComponent(
    composeEnvironment: StartedDockerComposeEnvironment,
    componentName: string,
    waitBetweenStopAndStart: number,
  ) {
    await composeEnvironment.getContainer(componentName).stop({ remove: false });
    await Commons.sleep(waitBetweenStopAndStart);
    await composeEnvironment.getContainer(componentName).restart();
  }

  public static importGraphQL(filePath: string, fileName: string) {
    return fs.readFileSync(path.resolve(filePath, fileName), 'utf-8');
  }
}
