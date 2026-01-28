import { create } from 'zustand';
import type { ConnectionConfig, DriverInfo, Table } from '@/types';

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
  setTables: (tables: Table[]) => void;

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
    set({
      connectionConfig: config,
      isConnected,
      loadedFrom,
      lastUpdatedAt: lastUpdatedAt ?? config.updated_at ?? null,
    }),
  loadedFrom: null,
  lastUpdatedAt: null,
  setLoadedFrom: (value, lastUpdatedAt = null) =>
    set({
      loadedFrom: value,
      lastUpdatedAt,
    }),
  disconnect: () =>
    set({
      connectionConfig: null,
      isConnected: false,
      currentStep: 0,
      selectedTables: [],
      loadedFrom: null,
      lastUpdatedAt: null,
      tables: [],
    }),
  driverInfo: null,
  setDriverInfo: (info) => set({ driverInfo: info }),
  tables: [],
  setTables: (tables) => set({ tables }),

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
