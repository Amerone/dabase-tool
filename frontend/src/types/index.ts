export type ConfigSource = 'sqlite' | 'env';
export type DriverSource = 'Bundled' | 'Env' | 'System';

export interface ConnectionConfig {
  host: string;
  port: number;
  username: string;
  password: string;
  schema: string;
  export_schema?: string;
  source?: ConfigSource;
  updated_at?: string;
}

export interface StoredConnectionResponse {
  config: ConnectionConfig;
  source: ConfigSource;
  updated_at?: string;
}

export interface Table {
  name: string;
  comment?: string;
  row_count?: number;
}

export interface Column {
  name: string;
  data_type: string;
  length?: number;
  nullable: boolean;
  comment?: string;
}

export interface Index {
  name: string;
  columns: string[];
  unique: boolean;
}

export interface TableDetails {
  name: string;
  comment?: string;
  columns: Column[];
  primary_keys: string[];
  indexes: Index[];
}

export interface ExportRequest {
  config: ConnectionConfig;
  export_schema?: string;
  tables: string[];
  include_ddl: boolean;
  include_data: boolean;
  batch_size?: number;
}

export interface ExportResponse {
  success: boolean;
  message: string;
  file_path?: string;
}

export interface ApiResponse<T> {
  success: boolean;
  data?: T;
  error?: string;
}

export interface TestConnectionResponse {
  success: boolean;
  message: string;
}

export interface DriverInfo {
  path: string;
  source: DriverSource;
}
