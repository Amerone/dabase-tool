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
  Table,
  TableDetails,
  TestConnectionResponse,
} from '../types';

const isTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

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

export const listTables = async (config: ConnectionConfig): Promise<ApiResponse<Table[]>> => {
  try {
    const api = await getApi();
    const response = await api.post<ApiResponse<Table[]>>('/tables', normalizeConfig(config));
    return response.data;
  } catch (error) {
    return { success: false, error: extractApiError(error, 'Failed to load tables') };
  }
};

export const getTableDetails = async (
  config: ConnectionConfig,
  tableName: string
): Promise<ApiResponse<TableDetails>> => {
  try {
    const api = await getApi();
    const response = await api.post<ApiResponse<TableDetails>>(
      `/tables/${encodeURIComponent(tableName)}/details`,
      normalizeConfig(config)
    );
    return response.data;
  } catch (error) {
    return { success: false, error: extractApiError(error, 'Failed to load table details') };
  }
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

export const getExportDirectory = async (): Promise<ApiResponse<string>> => {
  try {
    const api = await getApi();
    const response = await api.get<ApiResponse<string>>('/export/directory');
    return response.data;
  } catch (error) {
    return { success: false, error: extractApiError(error, 'Failed to load export directory') };
  }
};

export const getExportCapabilities = async (
  sourceDbType: DbType,
  targetDialect?: DbType
): Promise<ApiResponse<ExportCapabilityReport>> => {
  try {
    const api = await getApi();
    const response = await api.get<ApiResponse<ExportCapabilityReport>>('/export/capabilities', {
      params: {
        source_db_type: sourceDbType,
        target_dialect: targetDialect ?? sourceDbType,
      },
    });
    return response.data;
  } catch (error) {
    return { success: false, error: extractApiError(error, 'Failed to load export capabilities') };
  }
};
