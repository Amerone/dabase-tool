import axios from 'axios';
import { invoke } from '@tauri-apps/api/core';
import type {
  ApiResponse,
  ConnectionConfig,
  DbType,
  DriverInfo,
  ExportCapabilityReport,
  ExportRequest,
  ExportResponse,
  NamedConnectionResponse,
  StoredConnectionResponse,
  TableIdentifier,
  Table,
  TableDetails,
  TestConnectionResponse,
} from '../types';
import { buildConnectionKey } from '@/utils/connectionKey';

const isTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

type CachedResponse<T> = {
  expiresAt: number;
  value: ApiResponse<T>;
};

type FetchOptions = {
  forceRefresh?: boolean;
};

const TABLE_LIST_TTL_MS = 30_000;
const TABLE_DETAILS_TTL_MS = 120_000;
const EXPORT_CAPABILITY_TTL_MS = 300_000;
const MAX_CACHE_ENTRIES = 300;
const TABLE_DETAILS_BATCH_SIZE = 200;
const SINGLE_DETAIL_FALLBACK_CONCURRENCY = 6;

const tableListCache = new Map<string, CachedResponse<Table[]>>();
const tableListInflight = new Map<string, Promise<ApiResponse<Table[]>>>();

const tableDetailsCache = new Map<string, CachedResponse<TableDetails>>();
const tableDetailsInflight = new Map<string, Promise<ApiResponse<TableDetails>>>();
const tableDetailsBatchInflight = new Map<string, Promise<ApiResponse<TableDetails[]>>>();

const exportCapabilityCache = new Map<string, CachedResponse<ExportCapabilityReport>>();
const exportCapabilityInflight = new Map<string, Promise<ApiResponse<ExportCapabilityReport>>>();

function enforceCacheLimit<T>(cache: Map<string, CachedResponse<T>>) {
  while (cache.size > MAX_CACHE_ENTRIES) {
    const oldestKey = cache.keys().next().value as string | undefined;
    if (oldestKey === undefined) {
      break;
    }
    cache.delete(oldestKey);
  }
}

function chunkArray<T>(items: T[], size: number): T[][] {
  const chunks: T[][] = [];
  for (let index = 0; index < items.length; index += size) {
    chunks.push(items.slice(index, index + size));
  }
  return chunks;
}

function normalizeTableName(tableName: string): string {
  return tableName.trim();
}

function normalizeTableRef(
  table: string | TableIdentifier,
  fallbackSchema: string
): TableIdentifier {
  if (typeof table === 'string') {
    return {
      schema: fallbackSchema,
      name: normalizeTableName(table),
    };
  }

  return {
    schema: table.schema.trim() || fallbackSchema,
    name: normalizeTableName(table.name),
  };
}

function tableRefKey(table: TableIdentifier): string {
  return `${table.schema.trim()}|${normalizeTableName(table.name)}`;
}

function makeTableDetailsCacheKey(configKey: string, schema: string, tableName: string): string {
  return `${configKey}|${schema.trim()}|${normalizeTableName(tableName)}`;
}

function rememberTableDetails(
  configKey: string,
  schema: string,
  tableName: string,
  details: TableDetails,
  combined?: Map<string, TableDetails>
) {
  const normalizedName = normalizeTableName(tableName);
  if (!normalizedName) {
    return;
  }

  const normalizedSchema = schema.trim();
  tableDetailsCache.set(makeTableDetailsCacheKey(configKey, normalizedSchema, normalizedName), {
    expiresAt: Date.now() + TABLE_DETAILS_TTL_MS,
    value: { success: true, data: details },
  });
  combined?.set(`${normalizedSchema}|${normalizedName}`, details);
}

function makeTableDetailsBatchInflightKey(configKey: string, tableRefs: TableIdentifier[]): string {
  const normalized = tableRefs.map(tableRefKey);
  return `${configKey}|${normalized.join(',')}`;
}

function buildOrderedTableDetails(
  requestedRefs: TableIdentifier[],
  byName: Map<string, TableDetails>
): TableDetails[] | null {
  const ordered: TableDetails[] = [];
  for (const table of requestedRefs) {
    const details = byName.get(tableRefKey(table));
    if (!details) {
      return null;
    }
    ordered.push(details);
  }
  return ordered;
}

async function resolveBaseUrl() {
  if (isTauri()) {
    try {
      const backend = await invoke<string>('backend_base_url');
      return `${backend.replace(/\/$/, '')}/api`;
    } catch (err) {
      console.error('Failed to resolve backend URL from Tauri', err);
    }
  }
  const envBase = import.meta.env.VITE_API_BASE_URL as string | undefined;
  return envBase ?? '/api';
}

async function createApiClient() {
  const client = axios.create({ timeout: 30000 });
  client.defaults.baseURL = await resolveBaseUrl();
  return client;
}

const apiPromise = createApiClient();

async function getApi() {
  return apiPromise;
}

function normalizeConfig(config: ConnectionConfig): ConnectionConfig {
  return {
    ...config,
    db_type: config.db_type ?? 'dm8',
  };
}

function extractApiError(error: unknown, fallback: string): string {
  if (axios.isAxiosError(error)) {
    const payload = error.response?.data as ApiResponse<unknown> | undefined;
    if (payload?.error) {
      return payload.error;
    }
    if (typeof error.response?.data === 'string' && error.response.data.trim()) {
      return error.response.data;
    }
    if (error.message) {
      return error.message;
    }
  }
  if (error instanceof Error && error.message) {
    return error.message;
  }
  if (typeof error === 'string' && error.trim()) {
    return error;
  }
  return fallback;
}

export const testConnection = async (
  config: ConnectionConfig
): Promise<ApiResponse<TestConnectionResponse>> => {
  try {
    const api = await getApi();
    const response = await api.post<ApiResponse<TestConnectionResponse>>(
      '/connection/test',
      normalizeConfig(config)
    );
    return response.data;
  } catch (error) {
    return { success: false, error: extractApiError(error, 'Connection test failed') };
  }
};

export const getSavedConnection = async (
  dbType?: DbType
): Promise<ApiResponse<StoredConnectionResponse>> => {
  try {
    const api = await getApi();
    const response = await api.get<ApiResponse<StoredConnectionResponse>>('/config/connection', {
      params: dbType ? { db_type: dbType } : undefined,
    });
    return response.data;
  } catch (error) {
    return { success: false, error: extractApiError(error, 'Failed to load saved connection') };
  }
};

export const listConnections = async (): Promise<ApiResponse<NamedConnectionResponse[]>> => {
  try {
    const api = await getApi();
    const response = await api.get<ApiResponse<NamedConnectionResponse[]>>('/config/connections');
    return response.data;
  } catch (error) {
    return { success: false, error: extractApiError(error, 'Failed to list connections') };
  }
};

export const saveNamedConnection = async (
  name: string,
  config: ConnectionConfig
): Promise<ApiResponse<NamedConnectionResponse>> => {
  try {
    const api = await getApi();
    const response = await api.post<ApiResponse<NamedConnectionResponse>>('/config/connections', {
      name,
      config: normalizeConfig(config),
    });
    return response.data;
  } catch (error) {
    return { success: false, error: extractApiError(error, 'Failed to save named connection') };
  }
};

export const deleteConnection = async (id: number): Promise<ApiResponse<boolean>> => {
  try {
    const api = await getApi();
    const response = await api.delete<ApiResponse<boolean>>(`/config/connections/${id}`);
    return response.data;
  } catch (error) {
    return { success: false, error: extractApiError(error, 'Failed to delete connection') };
  }
};

export const saveConnection = async (
  config: ConnectionConfig
): Promise<ApiResponse<StoredConnectionResponse>> => {
  try {
    const api = await getApi();
    const response = await api.post<ApiResponse<StoredConnectionResponse>>(
      '/config/connection',
      normalizeConfig(config)
    );
    return response.data;
  } catch (error) {
    return { success: false, error: extractApiError(error, 'Failed to save connection') };
  }
};

export const listTables = async (
  config: ConnectionConfig,
  options: FetchOptions = {}
): Promise<ApiResponse<Table[]>> => {
  const normalizedConfig = normalizeConfig(config);
  const cacheKey = buildConnectionKey(normalizedConfig);
  const now = Date.now();
  const inflightKey = `${cacheKey}|refresh:${Boolean(options.forceRefresh)}`;
  const inflight = tableListInflight.get(inflightKey);
  if (inflight) {
    return inflight;
  }

  if (!options.forceRefresh) {
    const cached = tableListCache.get(cacheKey);
    if (cached && cached.expiresAt > now) {
      return cached.value;
    }
  }

  const request = (async () => {
    try {
      const api = await getApi();
      const requestPayload = {
        ...normalizedConfig,
        force_refresh: Boolean(options.forceRefresh),
      };
      const response = await api.post<ApiResponse<Table[]>>('/tables', requestPayload);
      const responsePayload = response.data;
      if (responsePayload.success) {
        tableListCache.set(cacheKey, {
          expiresAt: Date.now() + TABLE_LIST_TTL_MS,
          value: responsePayload,
        });
        enforceCacheLimit(tableListCache);
      }
      return responsePayload;
    } catch (error) {
      return { success: false, error: extractApiError(error, 'Failed to load tables') };
    } finally {
      tableListInflight.delete(inflightKey);
    }
  })();

  tableListInflight.set(inflightKey, request);
  return request;
};

export const getTableDetails = async (
  config: ConnectionConfig,
  table: string | TableIdentifier,
  options: FetchOptions = {}
): Promise<ApiResponse<TableDetails>> => {
  const normalizedConfig = normalizeConfig(config);
  const tableRef = normalizeTableRef(table, normalizedConfig.schema);
  const normalizedTableName = tableRef.name.trim();
  if (!normalizedTableName) {
    return { success: false, error: 'Table name is required' };
  }

  const configKey = buildConnectionKey(normalizedConfig);
  const cacheKey = makeTableDetailsCacheKey(configKey, tableRef.schema, normalizedTableName);
  const now = Date.now();
  const inflightKey = `${cacheKey}|refresh:${Boolean(options.forceRefresh)}`;
  const inflight = tableDetailsInflight.get(inflightKey);
  if (inflight) {
    return inflight;
  }

  if (!options.forceRefresh) {
    const cached = tableDetailsCache.get(cacheKey);
    if (cached && cached.expiresAt > now) {
      return cached.value;
    }
  }

  const request = (async () => {
    try {
      const api = await getApi();
      const payload = {
        ...normalizedConfig,
        table_schema: tableRef.schema,
        force_refresh: Boolean(options.forceRefresh),
      };
      const response = await api.post<ApiResponse<TableDetails>>(
        `/tables/${encodeURIComponent(normalizedTableName)}/details`,
        payload
      );
      const result = response.data;
      if (result.success && result.data) {
        tableDetailsCache.set(cacheKey, {
          expiresAt: Date.now() + TABLE_DETAILS_TTL_MS,
          value: result,
        });
        enforceCacheLimit(tableDetailsCache);
      }
      return result;
    } catch (error) {
      return { success: false, error: extractApiError(error, 'Failed to load table details') };
    } finally {
      tableDetailsInflight.delete(inflightKey);
    }
  })();

  tableDetailsInflight.set(inflightKey, request);
  return request;
};

function normalizeTableRefs(
  tables: Array<string | TableIdentifier>,
  fallbackSchema: string
): TableIdentifier[] {
  const seen = new Set<string>();
  const normalized: TableIdentifier[] = [];
  for (const table of tables) {
    const ref = normalizeTableRef(table, fallbackSchema);
    if (!ref.name) {
      continue;
    }
    const key = tableRefKey(ref);
    if (seen.has(key)) {
      continue;
    }
    seen.add(key);
    normalized.push(ref);
  }
  return normalized;
}

export const getTableDetailsBatch = async (
  config: ConnectionConfig,
  tables: Array<string | TableIdentifier>,
  options: FetchOptions = {}
): Promise<ApiResponse<TableDetails[]>> => {
  const normalizedConfig = normalizeConfig(config);
  const requestedRefs = normalizeTableRefs(tables, normalizedConfig.schema);
  if (requestedRefs.length === 0) {
    return { success: true, data: [] };
  }

  const configKey = buildConnectionKey(normalizedConfig);
  const now = Date.now();
  const fromCache = new Map<string, TableDetails>();
  const missingNames: string[] = [];

  if (!options.forceRefresh) {
    for (const table of requestedRefs) {
      const detailKey = makeTableDetailsCacheKey(configKey, table.schema, table.name);
      const cached = tableDetailsCache.get(detailKey);
      if (cached && cached.expiresAt > now && cached.value.success && cached.value.data) {
        fromCache.set(tableRefKey(table), cached.value.data);
      } else {
        missingNames.push(tableRefKey(table));
      }
    }
  } else {
    missingNames.push(...requestedRefs.map(tableRefKey));
  }

  if (missingNames.length === 0) {
    const ordered = buildOrderedTableDetails(requestedRefs, fromCache);
    if (ordered) {
      return { success: true, data: ordered };
    }
    return {
      success: false,
      error: 'Cached table details are incomplete',
    };
  }

  const missingRefs = requestedRefs.filter((table) => missingNames.includes(tableRefKey(table)));
  const batchKey = `${makeTableDetailsBatchInflightKey(configKey, requestedRefs)}|refresh:${Boolean(options.forceRefresh)}`;
  const inflight = tableDetailsBatchInflight.get(batchKey);
  if (inflight) {
    return inflight;
  }

  const request = (async () => {
    const fetchSingleDetails = async (refs: TableIdentifier[], forceRefresh: boolean) => {
      const byRequestedName = new Map<string, TableDetails>();
      let hasFailure = false;
      let nextIndex = 0;
      const workerCount = Math.min(SINGLE_DETAIL_FALLBACK_CONCURRENCY, refs.length);

      await Promise.all(
        Array.from({ length: workerCount }, async () => {
          while (nextIndex < refs.length && !hasFailure) {
            const tableRef = refs[nextIndex++];
            const result = await getTableDetails(normalizedConfig, tableRef, { forceRefresh });
            if (!result.success || !result.data) {
              hasFailure = true;
              return;
            }
            byRequestedName.set(tableRefKey(tableRef), result.data);
          }
        })
      );

      return hasFailure ? null : byRequestedName;
    };

    try {
      const api = await getApi();
      const combined = new Map<string, TableDetails>(fromCache);
      let batchError: string | undefined;

      const refsBySchema = new Map<string, TableIdentifier[]>();
      for (const table of missingRefs) {
        const refs = refsBySchema.get(table.schema) ?? [];
        refs.push(table);
        refsBySchema.set(table.schema, refs);
      }

      for (const [schema, refs] of refsBySchema) {
        for (const chunk of chunkArray(refs, TABLE_DETAILS_BATCH_SIZE)) {
          const payload = {
            ...normalizedConfig,
            table_schema: schema,
            tables: chunk.map((table) => table.name),
            force_refresh: Boolean(options.forceRefresh),
          };
          const response = await api.post<ApiResponse<TableDetails[]>>(
            '/tables/details/batch',
            payload
          );

          if (response.data.success && response.data.data) {
            const canAliasByOrder = response.data.data.length === chunk.length;
            response.data.data.forEach((detail, index) => {
              rememberTableDetails(configKey, schema, detail.name, detail, combined);

              const requestedName = canAliasByOrder ? chunk[index]?.name : undefined;
              if (
                requestedName &&
                normalizeTableName(requestedName) !== normalizeTableName(detail.name)
              ) {
                rememberTableDetails(configKey, schema, requestedName, detail, combined);
              }
            });
          } else {
            batchError = response.data.error || batchError;
          }
        }
      }
      enforceCacheLimit(tableDetailsCache);

      const unresolved = requestedRefs.filter((table) => !combined.has(tableRefKey(table)));
      if (unresolved.length > 0) {
        // Preserve old behavior by falling back to per-table API when batch data is incomplete.
        const fallbackDetails = await fetchSingleDetails(unresolved, true);
        if (!fallbackDetails) {
          return {
            success: false,
            error: batchError || 'Failed to load table details batch',
          };
        }
        for (const [requestedName, detail] of fallbackDetails) {
          combined.set(requestedName, detail);
        }
      }

      const ordered = buildOrderedTableDetails(requestedRefs, combined);
      if (!ordered) {
        return {
          success: false,
          error: batchError || 'Failed to load table details batch',
        };
      }

      return {
        success: true,
        data: ordered,
      };
    } catch (error) {
      const fallbackDetails = await fetchSingleDetails(missingRefs, Boolean(options.forceRefresh));
      if (fallbackDetails) {
        const combined = new Map<string, TableDetails>(fromCache);
        for (const [requestedName, detail] of fallbackDetails) {
          combined.set(requestedName, detail);
        }
        const ordered = buildOrderedTableDetails(requestedRefs, combined);
        if (ordered) {
          return {
            success: true,
            data: ordered,
          };
        }
      }

      return {
        success: false,
        error: extractApiError(error, 'Failed to load table details batch'),
      };
    } finally {
      tableDetailsBatchInflight.delete(batchKey);
    }
  })();

  tableDetailsBatchInflight.set(batchKey, request);
  return request;
};

export const getExportCapabilities = async (
  sourceDbType: DbType,
  targetDialect?: DbType,
  options: FetchOptions = {}
): Promise<ApiResponse<ExportCapabilityReport>> => {
  const key = `${sourceDbType}|${targetDialect ?? sourceDbType}`;
  const now = Date.now();
  const inflightKey = `${key}|refresh:${Boolean(options.forceRefresh)}`;
  const inflight = exportCapabilityInflight.get(inflightKey);
  if (inflight) {
    return inflight;
  }

  if (!options.forceRefresh) {
    const cached = exportCapabilityCache.get(key);
    if (cached && cached.expiresAt > now) {
      return cached.value;
    }
  }

  const request = (async () => {
    try {
      const api = await getApi();
      const response = await api.get<ApiResponse<ExportCapabilityReport>>('/export/capabilities', {
        params: {
          source_db_type: sourceDbType,
          target_dialect: targetDialect ?? sourceDbType,
        },
      });
      const payload = response.data;
      if (payload.success && payload.data) {
        exportCapabilityCache.set(key, {
          expiresAt: Date.now() + EXPORT_CAPABILITY_TTL_MS,
          value: payload,
        });
        enforceCacheLimit(exportCapabilityCache);
      }
      return payload;
    } catch (error) {
      return { success: false, error: extractApiError(error, 'Failed to load export capabilities') };
    } finally {
      exportCapabilityInflight.delete(inflightKey);
    }
  })();

  exportCapabilityInflight.set(inflightKey, request);
  return request;
};

export const clearApiCaches = () => {
  tableListCache.clear();
  tableListInflight.clear();
  tableDetailsCache.clear();
  tableDetailsInflight.clear();
  tableDetailsBatchInflight.clear();
  exportCapabilityCache.clear();
  exportCapabilityInflight.clear();
};

export const exportDDL = async (
  request: ExportRequest
): Promise<ApiResponse<ExportResponse>> => {
  try {
    const api = await getApi();
    const payload: ExportRequest = {
      ...request,
      config: normalizeConfig(request.config),
    };
    const response = await api.post<ApiResponse<ExportResponse>>('/export/ddl', payload, {
      timeout: 0,
    });
    return response.data;
  } catch (error) {
    return { success: false, error: extractApiError(error, 'Failed to export DDL') };
  }
};

export const exportData = async (
  request: ExportRequest
): Promise<ApiResponse<ExportResponse>> => {
  try {
    const api = await getApi();
    const payload: ExportRequest = {
      ...request,
      config: normalizeConfig(request.config),
    };
    const response = await api.post<ApiResponse<ExportResponse>>('/export/data', payload, {
      timeout: 0,
    });
    return response.data;
  } catch (error) {
    return { success: false, error: extractApiError(error, 'Failed to export data') };
  }
};

export const getDriverInfo = async (): Promise<DriverInfo | null> => {
  if (!isTauri()) {
    return null;
  }
  try {
    return await invoke<DriverInfo>('driver_info');
  } catch (error) {
    console.warn('Failed to load driver info', error);
    return null;
  }
};

export const chooseExportDirectory = async (
  initialDirectory?: string
): Promise<ApiResponse<string | null>> => {
  if (!isTauri()) {
    return {
      success: false,
      error: '桌面端可打开系统目录选择器，浏览器模式请手动输入导出目录',
    };
  }

  try {
    const selected = await invoke<string | null>('choose_export_directory', {
      initialDirectory,
    });
    return { success: true, data: selected };
  } catch (error) {
    return { success: false, error: extractApiError(error, 'Failed to choose export directory') };
  }
};

export const getExportDirectory = async (): Promise<ApiResponse<string>> => {
  try {
    const api = await getApi();
    const response = await api.get<ApiResponse<string>>('/export/directory');
    return response.data;
  } catch (error) {
    return { success: false, error: extractApiError(error, 'Failed to load export directory') };
  }
};

export const saveExportDirectory = async (directory: string): Promise<ApiResponse<string>> => {
  try {
    const api = await getApi();
    const response = await api.post<ApiResponse<string>>('/export/directory', { directory });
    return response.data;
  } catch (error) {
    return { success: false, error: extractApiError(error, 'Failed to save export directory') };
  }
};
