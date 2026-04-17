import { useEffect, useMemo, useRef, useState } from 'react';
import { CodeOutlined, DeleteOutlined, ReloadOutlined } from '@ant-design/icons';
import { Button, Collapse, Space, Spin, Tag } from 'antd';

import type { TableDetails } from '@/types';
import { getTableDetailsBatch } from '@/services/api';
import { useExportStore } from '@/store/useExportStore';
import { buildConnectionKey } from '@/utils/connectionKey';
import { SectionHeader } from './common/SectionHeader';
import { TechCard } from './common/TechCard';

const { Panel } = Collapse;

function pruneRecordBySelected<T>(
  prev: Record<string, T>,
  selected: Set<string>
): Record<string, T> {
  let changed = false;
  const next: Record<string, T> = {};
  for (const [tableName, value] of Object.entries(prev)) {
    if (selected.has(tableName)) {
      next[tableName] = value;
    } else {
      changed = true;
    }
  }
  return changed ? next : prev;
}

export default function TableSelector() {
  const config = useExportStore((state) => state.connectionConfig);
  const selectedTables = useExportStore((state) => state.selectedTables);
  const toggleTable = useExportStore((state) => state.toggleTable);
  const tables = useExportStore((state) => state.tables);

  const [detailsMap, setDetailsMap] = useState<Record<string, TableDetails | null>>({});
  const [loadingMap, setLoadingMap] = useState<Record<string, boolean>>({});
  const [activeKeys, setActiveKeys] = useState<string[]>([]);
  const inFlightRef = useRef<Set<string>>(new Set());
  const configKeyRef = useRef<string | null>(null);
  const previousSelectedCountRef = useRef(0);
  const nextRequestTokenRef = useRef(0);
  const requestTokenRef = useRef<Record<string, number>>({});

  const configKey = useMemo(() => {
    if (!config) {
      return null;
    }
    return buildConnectionKey(config);
  }, [config]);

  const summaryMap = useMemo(() => {
    return new Map(tables.map((table) => [table.name, table]));
  }, [tables]);

  const fetchDetailsBatch = async (tableNames: string[], force = false) => {
    if (!config) {
      return;
    }

    const pendingNames = Array.from(new Set(tableNames))
      .map((name) => name.trim())
      .filter((name) => Boolean(name))
      .filter((name) => force || !inFlightRef.current.has(name))
      .filter((name) => force || detailsMap[name] === undefined);

    if (pendingNames.length === 0) {
      return;
    }

    const requestTokens: Record<string, number> = {};
    for (const tableName of pendingNames) {
      const token = ++nextRequestTokenRef.current;
      requestTokens[tableName] = token;
      requestTokenRef.current[tableName] = token;
      inFlightRef.current.add(tableName);
    }
    setLoadingMap((prev) => {
      const next = { ...prev };
      for (const tableName of pendingNames) {
        next[tableName] = true;
      }
      return next;
    });
    const requestConfigKey = configKey;

    try {
      const response = await getTableDetailsBatch(config, pendingNames, { forceRefresh: force });
      if (configKeyRef.current !== requestConfigKey) {
        return;
      }
      const byRequestedName = new Map<string, TableDetails>();
      if (response.success && response.data) {
        response.data.forEach((details, index) => {
          const tableName = pendingNames[index];
          if (tableName) {
            byRequestedName.set(tableName, details);
          }
        });
      }

      setDetailsMap((prev) => {
        const next = { ...prev };
        for (const tableName of pendingNames) {
          if (requestTokenRef.current[tableName] !== requestTokens[tableName]) {
            continue;
          }
          const detail = byRequestedName.get(tableName);
          next[tableName] = detail ?? null;
        }
        return next;
      });
    } catch {
      if (configKeyRef.current !== requestConfigKey) {
        return;
      }
      setDetailsMap((prev) => {
        const next = { ...prev };
        for (const tableName of pendingNames) {
          if (requestTokenRef.current[tableName] !== requestTokens[tableName]) {
            continue;
          }
          next[tableName] = null;
        }
        return next;
      });
    } finally {
      if (configKeyRef.current === requestConfigKey) {
        setLoadingMap((prev) => {
          const next = { ...prev };
          for (const tableName of pendingNames) {
            if (requestTokenRef.current[tableName] !== requestTokens[tableName]) {
              continue;
            }
            next[tableName] = false;
          }
          return next;
        });
      }
      for (const tableName of pendingNames) {
        if (requestTokenRef.current[tableName] === requestTokens[tableName]) {
          delete requestTokenRef.current[tableName];
          inFlightRef.current.delete(tableName);
        }
      }
    }
  };

  useEffect(() => {
    configKeyRef.current = configKey;
    setDetailsMap({});
    setLoadingMap({});
    setActiveKeys([]);
    inFlightRef.current.clear();
    requestTokenRef.current = {};
  }, [configKey]);

  useEffect(() => {
    const selected = new Set(selectedTables);
    const wasShrinking = selectedTables.length < previousSelectedCountRef.current;
    previousSelectedCountRef.current = selectedTables.length;

    if (wasShrinking) {
      setDetailsMap((prev) => pruneRecordBySelected(prev, selected));
      setLoadingMap((prev) => pruneRecordBySelected(prev, selected));
    }

    setActiveKeys((prev) => {
      const next = prev.filter((tableName) => selected.has(tableName));
      return next.length === prev.length ? prev : next;
    });
  }, [selectedTables]);

  const handleCollapseChange = (keys: string[] | string) => {
    const nextKeys = Array.isArray(keys) ? keys : [keys];
    setActiveKeys(nextKeys);
    void fetchDetailsBatch(nextKeys);
  };

  if (selectedTables.length === 0) {
    return null;
  }

  return (
    <TechCard className="selected-table-card" delay={180} style={{ marginTop: 24 }}>
      <SectionHeader title="已选清单" subtitle={`共 ${selectedTables.length} 张表`} />

      <div className="selected-table-list">
        <Collapse
          ghost
          destroyInactivePanel
          activeKey={activeKeys}
          onChange={handleCollapseChange}
          expandIcon={({ isActive }) => (
            <CodeOutlined rotate={isActive ? 90 : 0} style={{ color: '#13c2c2' }} />
          )}
        >
          {selectedTables.map((tableName) => {
            const summary = summaryMap.get(tableName);
            const details = detailsMap[tableName];
            const loading = loadingMap[tableName];

            return (
              <Panel
                header={
                  <div className="selected-table-panel-header">
                    <span className="selected-table-name" title={tableName}>
                      {tableName}
                    </span>
                    <Space className="selected-table-actions" size={6}>
                      {summary && (
                        <Tag className="selected-table-tag">
                          {(summary.row_count ?? 0).toLocaleString()} rows
                        </Tag>
                      )}
                      <Button
                        type="text"
                        size="small"
                        danger
                        icon={<DeleteOutlined />}
                        onClick={(event) => {
                          event.stopPropagation();
                          toggleTable(tableName);
                        }}
                      />
                    </Space>
                  </div>
                }
                key={tableName}
                extra={
                  <Button
                    type="link"
                    size="small"
                    icon={<ReloadOutlined />}
                    onClick={(event) => {
                      event.stopPropagation();
                      void fetchDetailsBatch([tableName], true);
                    }}
                  />
                }
                className="selected-table-panel"
              >
                {loading && (
                  <div className="selected-table-loading">
                    <Spin size="small" />
                    <span>加载结构详情中...</span>
                  </div>
                )}

                {!loading && !details && (
                  <div className="selected-table-metadata">
                    <p>详情暂不可用，可点击右上角刷新重试。</p>
                  </div>
                )}

                {!loading && details && (
                  <div className="selected-table-metadata">
                    <p>
                      列数: <span>{details.columns.length}</span>
                    </p>
                    <p>
                      主键列: <span>{details.primary_keys.length}</span>
                    </p>
                    <p>
                      索引数: <span>{details.indexes.length}</span>
                    </p>
                    <p>
                      外键数: <span>{details.foreign_keys.length}</span>
                    </p>
                  </div>
                )}
              </Panel>
            );
          })}
        </Collapse>
      </div>
    </TechCard>
  );
}
