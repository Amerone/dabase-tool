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
  precision?: number;
  scale?: number;
  char_semantics?: string;
  nullable: boolean;
  comment?: string;
  default_value?: string;
  identity?: boolean;
  identity_start?: number;
  identity_increment?: number;
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
  unique_constraints: UniqueConstraint[];
  foreign_keys: ForeignKey[];
  check_constraints: CheckConstraint[];
  triggers: TriggerDefinition[];
}

export interface UniqueConstraint {
  name: string;
  columns: string[];
}

export interface CheckConstraint {
  name: string;
  condition: string;
}

export interface ForeignKey {
  name: string;
  columns: string[];
  referenced_table: string;
  referenced_columns: string[];
  delete_rule?: string;
}

export interface TriggerDefinition {
  name: string;
  table_name: string;
  timing: string;
  events: string[];
  each_row: boolean;
  body: string;
}

export interface ExportRequest {
  config: ConnectionConfig;
  export_schema?: string;
  export_compat?: string;
  tables: string[];
  include_ddl: boolean;
  include_data: boolean;
  batch_size?: number;
  drop_existing?: boolean;
  include_row_counts?: boolean;
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
