import { useCallback, useDeferredValue, useEffect, useMemo, useRef, useState } from 'react';
import type { Key } from 'react';
import { DatabaseOutlined, ReloadOutlined, SearchOutlined } from '@ant-design/icons';
import { Input, Space, Table, Tag, message } from 'antd';
import type { ColumnsType } from 'antd/es/table';

import type { Table as TableType } from '@/types';
import { listTables } from '@/services/api';
import { useExportStore } from '@/store/useExportStore';
import { buildConnectionKey } from '@/utils/connectionKey';
import { SectionHeader } from './common/SectionHeader';
import { TechButton } from './common/TechButton';
import { TechCard } from './common/TechCard';

function isSameSelection(left: string[], right: string[]) {
  return left.length === right.length && left.every((value, index) => value === right[index]);
}

export default function SchemaExplorer() {
  const config = useExportStore((state) => state.connectionConfig);
  const tables = useExportStore((state) => state.tables);
  const tablesConfigKey = useExportStore((state) => state.tablesConfigKey);
  const selectedTables = useExportStore((state) => state.selectedTables);
  const setSelectedTables = useExportStore((state) => state.setSelectedTables);
  const setStoreTables = useExportStore((state) => state.setTables);

  const [loading, setLoading] = useState(false);
  const [searchText, setSearchText] = useState('');
  const [pagination, setPagination] = useState({ current: 1, pageSize: 20 });
  const deferredSearch = useDeferredValue(searchText.trim().toLowerCase());
  const lastRequestIdRef = useRef(0);

  const configKey = useMemo(() => {
    if (!config) {
      return null;
    }
    return buildConnectionKey(config);
  }, [config]);

  const loadTables = useCallback(
    async (force = false) => {
      if (!config || !configKey) {
        return;
      }

      if (!force && tables.length > 0 && tablesConfigKey === configKey) {
        return;
      }

      const requestId = ++lastRequestIdRef.current;
      setLoading(true);
      try {
        const result = await listTables(config, { forceRefresh: force });
        if (requestId !== lastRequestIdRef.current) {
          return;
        }

        const liveConfig = useExportStore.getState().connectionConfig;
        const liveConfigKey = liveConfig ? buildConnectionKey(liveConfig) : null;
        if (liveConfigKey !== configKey) {
          return;
        }

        if (result.success && result.data) {
          const nextTables = result.data;
          setStoreTables(nextTables, configKey);

          const tableNames = new Set(nextTables.map((table) => table.name));
          const currentSelected = useExportStore.getState().selectedTables;
          const nextSelected = currentSelected.filter((name) => tableNames.has(name));
          if (nextSelected.length !== currentSelected.length) {
            setSelectedTables(nextSelected);
          }

          setPagination((prev) => ({ ...prev, current: 1 }));
          return;
        }
        message.error(result.error || '加载数据表失败');
      } catch {
        if (requestId !== lastRequestIdRef.current) {
          return;
        }
        message.error('加载数据表失败');
      } finally {
        if (requestId === lastRequestIdRef.current) {
          setLoading(false);
        }
      }
    },
    [config, configKey, setSelectedTables, setStoreTables, tables.length, tablesConfigKey]
  );

  useEffect(() => {
    setSearchText('');
    setPagination({ current: 1, pageSize: 20 });
    lastRequestIdRef.current += 1;
    setLoading(false);
  }, [configKey]);

  useEffect(() => {
    if (!config || !configKey) {
      return;
    }
    if (tables.length === 0 || tablesConfigKey !== configKey) {
      void loadTables();
    }
  }, [config, configKey, loadTables, tables.length, tablesConfigKey]);

  const filteredTables = useMemo(() => {
    if (!deferredSearch) {
      return tables;
    }
    return tables.filter((table) => table.name.toLowerCase().includes(deferredSearch));
  }, [deferredSearch, tables]);

  const visibleTableNames = useMemo(() => {
    return new Set(filteredTables.map((item) => item.name));
  }, [filteredTables]);

  const columns: ColumnsType<TableType> = useMemo(
    () => [
      {
        title: '表名',
        dataIndex: 'name',
        key: 'name',
        width: '38%',
        sorter: (a, b) => a.name.localeCompare(b.name),
        render: (text: string) => <span className="schema-table-name">{text}</span>,
      },
      {
        title: '注释',
        dataIndex: 'comment',
        key: 'comment',
        ellipsis: true,
        render: (text?: string) => <span className="schema-table-comment">{text || '-'}</span>,
      },
      {
        title: '估算行数',
        dataIndex: 'row_count',
        key: 'row_count',
        width: 140,
        align: 'right',
        render: (count?: number) => (
          <Tag className="schema-table-count">{(count ?? 0).toLocaleString()}</Tag>
        ),
      },
    ],
    []
  );

  const handleSelectAllVisible = () => {
    const current = new Set(selectedTables);
    const originalSize = current.size;
    for (const item of filteredTables) {
      current.add(item.name);
    }
    if (current.size === originalSize) {
      return;
    }
    setSelectedTables([...current]);
  };

  const handleClearVisible = () => {
    const next = selectedTables.filter((name) => !visibleTableNames.has(name));
    if (isSameSelection(next, selectedTables)) {
      return;
    }
    setSelectedTables(next);
  };

  const handleSelectionChange = (selectedRowKeys: Key[]) => {
    const keepHidden = selectedTables.filter((name) => !visibleTableNames.has(name));
    const next = Array.from(new Set([...keepHidden, ...(selectedRowKeys as string[])]));
    if (isSameSelection(next, selectedTables)) {
      return;
    }
    setSelectedTables(next);
  };

  if (!config) {
    return (
      <TechCard>
        <div className="panel-empty-state">
          <DatabaseOutlined className="panel-empty-icon" />
          <p>请先完成数据库连接，再加载数据表。</p>
        </div>
      </TechCard>
    );
  }

  return (
    <TechCard delay={80}>
      <div className="schema-toolbar">
        <SectionHeader title="数据表浏览器" subtitle={`发现 ${tables.length} 张表`} />
        <TechButton
          size="small"
          onClick={() => void loadTables(true)}
          icon={<ReloadOutlined />}
          loading={loading}
        >
          刷新
        </TechButton>
      </div>

      <Space direction="vertical" style={{ width: '100%', marginBottom: 16 }}>
        <div className="schema-actions">
          <Input
            placeholder="搜索表名..."
            prefix={<SearchOutlined />}
            value={searchText}
            onChange={(event) => setSearchText(event.target.value)}
            allowClear
            size="large"
          />
          <TechButton size="small" onClick={handleSelectAllVisible}>
            全选当前结果
          </TechButton>
          <TechButton size="small" danger onClick={handleClearVisible}>
            清空当前结果
          </TechButton>
        </div>
      </Space>

      <Table<TableType>
        rowKey="name"
        columns={columns}
        dataSource={filteredTables}
        loading={loading}
        virtual
        rowSelection={{
          selectedRowKeys: selectedTables,
          onChange: handleSelectionChange,
          preserveSelectedRowKeys: true,
        }}
        pagination={{
          current: pagination.current,
          pageSize: pagination.pageSize,
          showSizeChanger: true,
          showQuickJumper: true,
          onChange: (current, pageSize) => setPagination({ current, pageSize }),
          showTotal: (total) => `共 ${total} 条`,
        }}
        scroll={{ y: 460 }}
        size="middle"
      />
    </TechCard>
  );
}
