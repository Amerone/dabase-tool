export type ConfigSource = 'sqlite' | 'env';
export type DriverSource = 'Bundled' | 'Env' | 'System';
export type DbType = 'dm8' | 'mysql' | 'kingbase' | 'shentong';

export interface ConnectionConfig {
  db_type?: DbType;
  host: string;
  port: number;
  username: string;
  password: string;
  schema: string;
  export_schema?: string;
  database?: string;
  source?: ConfigSource;
  updated_at?: string;
}

export interface StoredConnectionResponse {
  config: ConnectionConfig;
  source: ConfigSource;
  updated_at?: string;
}

export interface NamedConnectionResponse {
  id: number;
  name: string;
  config: ConnectionConfig;
  updated_at: string;
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

export interface TableIdentifier {
  schema: string;
  name: string;
}

export interface ExportRequest {
  config: ConnectionConfig;
  target_dialect?: DbType;
  export_schema?: string;
  export_directory?: string;
  export_compat?: string;
  tables: TableIdentifier[];
  include_ddl: boolean;
  include_data: boolean;
  batch_size?: number;
  drop_existing?: boolean;
  include_row_counts?: boolean;
  strict_mode?: boolean;
  identifier_case?: string;
}

export type CapabilityLevel = 'none' | 'partial' | 'full';
export type ExportObjectKind =
  | 'ddl'
  | 'data'
  | 'columns'
  | 'primary_keys'
  | 'indexes'
  | 'unique_constraints'
  | 'foreign_keys'
  | 'check_constraints'
  | 'triggers'
  | 'sequences';

export interface ExportCapabilityEntry {
  object: ExportObjectKind;
  source_level: CapabilityLevel;
  target_level: CapabilityLevel;
  effective_level: CapabilityLevel;
  note?: string;
  reason_code?: string;
}

export interface ExportDataOptions {
  include_row_counts_supported: boolean;
  include_row_counts_note?: string;
}

export interface ExportCapabilityReport {
  source_db_type: DbType;
  target_dialect: DbType;
  entries: ExportCapabilityEntry[];
  data_options: ExportDataOptions;
}

export interface ExportResponse {
  success: boolean;
  message: string;
  file_path?: string;
  summary?: ExportExecutionSummary;
}

export interface ExportExecutionSummary {
  workload: ExportObjectKind;
  execution_path: string;
  duration_ms: number;
  rows_exported?: number;
  warnings: string[];
  skipped_objects: ExportSkippedObject[];
}

export interface ExportSkippedObject {
  object_kind: ExportObjectKind;
  object_name: string;
  reason_code?: string;
  message: string;
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
  drivers?: PackagedDriverInfo[];
}

export interface PackagedDriverInfo {
  database: string;
  path: string;
  source: DriverSource;
  required: boolean;
}
