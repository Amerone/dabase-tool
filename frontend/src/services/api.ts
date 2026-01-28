import axios from 'axios';
import { invoke } from '@tauri-apps/api/tauri';
import type {
  ConnectionConfig,
  Table,
  TableDetails,
  ExportRequest,
  ExportResponse,
  ApiResponse,
  TestConnectionResponse,
  StoredConnectionResponse,
  DriverInfo,
} from '../types';

const isTauri = () => typeof window !== 'undefined' && '__TAURI_IPC__' in window;

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

export const testConnection = async (
  config: ConnectionConfig
): Promise<ApiResponse<TestConnectionResponse>> => {
  try {
    const api = await getApi();
    const response = await api.post<ApiResponse<TestConnectionResponse>>(
      '/connection/test',
      config
    );
    return response.data;
  } catch (error) {
    return {
      success: false,
      error: error instanceof Error ? error.message : '连接测试失败',
    };
  }
};

export const getSavedConnection = async (): Promise<
  ApiResponse<StoredConnectionResponse>
> => {
  try {
    const api = await getApi();
    const response = await api.get<ApiResponse<StoredConnectionResponse>>(
      '/config/connection'
    );
    return response.data;
  } catch (error) {
    return {
      success: false,
      error: error instanceof Error ? error.message : '加载已保存配置失败',
    };
  }
};

export const saveConnection = async (
  config: ConnectionConfig
): Promise<ApiResponse<StoredConnectionResponse>> => {
  try {
    const api = await getApi();
    const response = await api.post<ApiResponse<StoredConnectionResponse>>(
      '/config/connection',
      config
    );
    return response.data;
  } catch (error) {
    return {
      success: false,
      error: error instanceof Error ? error.message : '保存配置失败',
    };
  }
};

export const listTables = async (
  config: ConnectionConfig
): Promise<ApiResponse<Table[]>> => {
  try {
    const api = await getApi();
    const response = await api.get<ApiResponse<Table[]>>('/tables', {
      params: config,
    });
    return response.data;
  } catch (error) {
    return {
      success: false,
      error: error instanceof Error ? error.message : '获取表列表失败',
    };
  }
};

export const getTableDetails = async (
  config: ConnectionConfig,
  tableName: string
): Promise<ApiResponse<TableDetails>> => {
  try {
    const api = await getApi();
    const response = await api.get<ApiResponse<TableDetails>>(
      `/tables/${tableName}/details`,
      {
        params: config,
      }
    );
    return response.data;
  } catch (error) {
    return {
      success: false,
      error: error instanceof Error ? error.message : '获取表详情失败',
    };
  }
};

export const exportDDL = async (
  request: ExportRequest
): Promise<ApiResponse<ExportResponse>> => {
  try {
    const api = await getApi();
    const response = await api.post<ApiResponse<ExportResponse>>(
      '/export/ddl',
      request
    );
    return response.data;
  } catch (error) {
    return {
      success: false,
      error: error instanceof Error ? error.message : '导出 DDL 失败',
    };
  }
};

export const exportData = async (
  request: ExportRequest
): Promise<ApiResponse<ExportResponse>> => {
  try {
    const api = await getApi();
    const response = await api.post<ApiResponse<ExportResponse>>(
      '/export/data',
      request
    );
    return response.data;
  } catch (error) {
    return {
      success: false,
      error: error instanceof Error ? error.message : '导出数据失败',
    };
  }
};

export const getDriverInfo = async (): Promise<DriverInfo | null> => {
  if (!isTauri()) return null;
  try {
    return await invoke<DriverInfo>('driver_info');
  } catch (error) {
    console.warn('Failed to load driver info', error);
    return null;
  }
};
