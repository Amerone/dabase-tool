import { create } from 'zustand';
import type { ConnectionConfig, DriverInfo, Table } from '@/types';
import { buildConnectionKey } from '@/utils/connectionKey';
import { clearApiCaches } from '@/services/api';

interface ExportState {
  // Connection
  connectionConfig: ConnectionConfig | null;
  isConnected: boolean;
  setConnectionConfig: (
    config: ConnectionConfig,
    loadedFrom?: 'saved' | 'manual',
    lastUpdatedAt?: string | null,
    isConnected?: boolean,
  ) => void;
  loadedFrom: 'saved' | 'manual' | null;
  lastUpdatedAt: string | null;
  setLoadedFrom: (value: 'saved' | 'manual' | null, lastUpdatedAt?: string | null) => void;
  disconnect: () => void;
  driverInfo: DriverInfo | null;
  setDriverInfo: (info: DriverInfo | null) => void;
  tables: Table[];
  tablesConfigKey: string | null;
  setTables: (tables: Table[], configKey?: string | null) => void;

  // Selection
  selectedTables: string[];
  setSelectedTables: (tables: string[]) => void;
  toggleTable: (tableName: string) => void;
  clearSelection: () => void;

  // Wizard UI State
  currentStep: number;
  setCurrentStep: (step: number) => void;
  nextStep: () => void;
  prevStep: () => void;
}

export const useExportStore = create<ExportState>((set) => ({
  // Connection
  connectionConfig: null,
  isConnected: false,
  setConnectionConfig: (config, loadedFrom = 'manual', lastUpdatedAt = null, isConnected = true) =>
    set((state) => {
      const normalizedConfig = {
        ...config,
        db_type: config.db_type ?? 'dm8',
      };
      const previousKey = state.connectionConfig ? buildConnectionKey(state.connectionConfig) : null;
      const nextKey = buildConnectionKey(normalizedConfig);
      const connectionChanged = previousKey !== null && previousKey !== nextKey;
      if (connectionChanged) {
        clearApiCaches();
      }

      return {
        connectionConfig: normalizedConfig,
        isConnected,
        loadedFrom,
        lastUpdatedAt: lastUpdatedAt ?? config.updated_at ?? null,
        ...(connectionChanged
          ? {
              selectedTables: [],
              tables: [],
              tablesConfigKey: null,
              currentStep: 0,
            }
          : {}),
      };
    }),
  loadedFrom: null,
  lastUpdatedAt: null,
  setLoadedFrom: (value, lastUpdatedAt = null) =>
    set({
      loadedFrom: value,
      lastUpdatedAt,
    }),
  disconnect: () =>
    {
      clearApiCaches();
      set({
        connectionConfig: null,
        isConnected: false,
        currentStep: 0,
        selectedTables: [],
        loadedFrom: null,
        lastUpdatedAt: null,
        tables: [],
        tablesConfigKey: null,
      });
    },
  driverInfo: null,
  setDriverInfo: (info) => set({ driverInfo: info }),
  tables: [],
  tablesConfigKey: null,
  setTables: (tables, configKey = null) => set({ tables, tablesConfigKey: configKey }),

  // Selection
  selectedTables: [],
  setSelectedTables: (tables) => set({ selectedTables: tables }),
  toggleTable: (tableName) =>
    set((state) => {
      const exists = state.selectedTables.includes(tableName);
      return {
        selectedTables: exists
          ? state.selectedTables.filter((t) => t !== tableName)
          : [...state.selectedTables, tableName],
      };
    }),
  clearSelection: () => set({ selectedTables: [] }),

  // Wizard UI State
  currentStep: 0,
  setCurrentStep: (step) => set({ currentStep: step }),
  nextStep: () => set((state) => ({ currentStep: state.currentStep + 1 })),
  prevStep: () => set((state) => ({ currentStep: Math.max(0, state.currentStep - 1) })),
}));
