import { createWriteStream, mkdirSync } from 'node:fs';
import { dirname } from 'node:path';
import pino from 'pino';

export const createLogger = (logPath: string): pino.Logger => {
  mkdirSync(dirname(logPath), { recursive: true });
  const level = 'info' as const;
  return pino(
    {
      level: process.env.LOG_LEVEL ?? 'debug',
      depthLimit: 20,
      timestamp: pino.stdTimeFunctions.isoTime,
      formatters: {
        level: (label) => {
          return { level: label.toUpperCase() };
        },
      },
      redact: { paths: ['pid', 'hostname'], remove: true },
    },
    pino.multistream([{ stream: createWriteStream(logPath, { flags: 'a' }), level }]),
  );
};
