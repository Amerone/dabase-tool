import type { Table, TableIdentifier } from '@/types';

const TABLE_KEY_SEPARATOR = '\u001f';

type TableLike = {
  schema?: string;
  name: string;
};

export function tableKey(table: TableLike, fallbackSchema = ''): string {
  const schema = (table.schema ?? fallbackSchema).trim();
  return `${schema}${TABLE_KEY_SEPARATOR}${table.name.trim()}`;
}

export function parseTableKey(key: string, fallbackSchema: string): TableIdentifier {
  const separatorIndex = key.indexOf(TABLE_KEY_SEPARATOR);
  if (separatorIndex >= 0) {
    return {
      schema: key.slice(0, separatorIndex).trim() || fallbackSchema,
      name: key.slice(separatorIndex + 1).trim(),
    };
  }

  return {
    schema: fallbackSchema,
    name: key.trim(),
  };
}

export function tableDisplayName(table: TableLike, fallbackSchema = ''): string {
  const schema = (table.schema ?? fallbackSchema).trim();
  const name = table.name.trim();
  return schema ? `${schema}.${name}` : name;
}

export function tableDisplayNameFromKey(key: string, fallbackSchema: string): string {
  return tableDisplayName(parseTableKey(key, fallbackSchema), fallbackSchema);
}

export function tableFromKey(
  key: string,
  tables: Table[],
  fallbackSchema: string
): Table | undefined {
  return tables.find((table) => tableKey(table, fallbackSchema) === key);
}
