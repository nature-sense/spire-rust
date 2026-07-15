// Logger that writes to stderr (never interferes with JSON-RPC on stdout)

const PREFIX = '[spire-ext]';

export enum LogLevel {
  Debug = 0,
  Info = 1,
  Warn = 2,
  Error = 3,
}

const currentLevel: LogLevel = (() => {
  const env = process.env.SPIRE_LOG_LEVEL?.toLowerCase();
  switch (env) {
    case 'debug': return LogLevel.Debug;
    case 'info':  return LogLevel.Info;
    case 'warn':  return LogLevel.Warn;
    case 'error': return LogLevel.Error;
    default:      return LogLevel.Info;
  }
})();

function log(level: LogLevel, levelName: string, ...args: unknown[]): void {
  if (level < currentLevel) return;
  const timestamp = new Date().toISOString();
  const prefix = `${timestamp} ${PREFIX} ${levelName}`;
  // Always write to stderr so JSON-RPC on stdout stays clean
  process.stderr.write(`${prefix} ${args.map(a => typeof a === 'string' ? a : JSON.stringify(a)).join(' ')}\n`);
}

export const logger = {
  debug: (...args: unknown[]) => log(LogLevel.Debug, 'DEBUG', ...args),
  info:  (...args: unknown[]) => log(LogLevel.Info,  'INFO',  ...args),
  warn:  (...args: unknown[]) => log(LogLevel.Warn,  'WARN',  ...args),
  error: (...args: unknown[]) => log(LogLevel.Error, 'ERROR', ...args),
};
