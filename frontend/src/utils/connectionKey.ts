export type ConnectionKeyInput = {
  db_type?: string;
  host: string;
  port: number;
  username: string;
  password: string;
  schema: string;
  database?: string;
};

function fnv1a32(value: string): string {
  let hash = 0x811c9dc5;
  for (let i = 0; i < value.length; i += 1) {
    hash ^= value.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(16).padStart(8, '0');
}

export function buildConnectionKey(config: ConnectionKeyInput): string {
  const passwordHash = fnv1a32(config.password ?? '');

  return [
    (config.db_type ?? 'dm8').trim().toLowerCase(),
    config.host.trim().toLowerCase(),
    Number(config.port) || 0,
    config.username.trim(),
    config.schema.trim(),
    (config.database ?? '').trim(),
    passwordHash,
  ].join('|');
}
