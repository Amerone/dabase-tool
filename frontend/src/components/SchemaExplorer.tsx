import { useCallback, useEffect, useState } from 'react'
import type { Key } from 'react'
import { Table, Input, Space, message, Tag } from 'antd'
import {
  SearchOutlined,
  DatabaseOutlined,
  ReloadOutlined,
  CheckSquareOutlined,
  DeleteOutlined,
} from '@ant-design/icons'
import type { Table as TableType } from '@/types'
import { listTables } from '@/services/api'
import { useExportStore } from '@/store/useExportStore'
import { TechCard } from './common/TechCard'
import { TechButton } from './common/TechButton'
import { SectionHeader } from './common/SectionHeader'

export default function SchemaExplorer() {
  const config = useExportStore((state) => state.connectionConfig)
  const selectedTables = useExportStore((state) => state.selectedTables)
  const setSelectedTables = useExportStore((state) => state.setSelectedTables)
  const setStoreTables = useExportStore((state) => state.setTables)

  const [tables, setTables] = useState<TableType[]>([])
  const [filteredTables, setFilteredTables] = useState<TableType[]>([])
  const [loading, setLoading] = useState(false)
  const [searchText, setSearchText] = useState('')
  const [pagination, setPagination] = useState({ current: 1, pageSize: 10 })

  const loadTables = useCallback(async () => {
    if (!config) {
      return
    }

    setLoading(true)
    try {
      const result = await listTables(config)
      if (result.success && result.data) {
        setTables(result.data)
        setFilteredTables(result.data)
        setStoreTables(result.data)
        setPagination((prev) => ({ ...prev, current: 1 }))
      } else {
        message.error(result.error || '扫描失败')
      }
    } catch {
      message.error('扫描失败')
    } finally {
      setLoading(false)
    }
  }, [config, setStoreTables])

  useEffect(() => {
    if (config) {
      loadTables()
    }
  }, [config, loadTables])

  useEffect(() => {
    const timer = setTimeout(() => {
      if (searchText) {
        const filtered = tables.filter((table) =>
          table.name.toLowerCase().includes(searchText.toLowerCase())
        )
        setFilteredTables(filtered)
        setPagination((prev) => ({ ...prev, current: 1 }))
      } else {
        setFilteredTables(tables)
        setPagination((prev) => ({ ...prev, current: 1 }))
      }
    }, 300)

    return () => clearTimeout(timer)
  }, [searchText, tables])

  const handleSelectAll = () => {
    const existing = new Set(selectedTables)
    const merged = [...selectedTables, ...filteredTables.map((t) => t.name).filter((n) => !existing.has(n))]
    setSelectedTables(merged)
  }

  const handleClearAll = () => {
    const filteredNames = new Set(filteredTables.map((t) => t.name))
    setSelectedTables(selectedTables.filter((name) => !filteredNames.has(name)))
  }

  const handleRowSelection = (selectedRowKeys: Key[]) => {
    const filteredNames = new Set(filteredTables.map((t) => t.name))
    const kept = selectedTables.filter((name) => !filteredNames.has(name))
    setSelectedTables([...kept, ...(selectedRowKeys as string[])])
  }

  const columns = [
    {
      title: '表名',
      dataIndex: 'name',
      key: 'name',
      sorter: (a: TableType, b: TableType) => a.name.localeCompare(b.name),
      render: (text: string) => (
        <span style={{ fontFamily: 'JetBrains Mono', color: '#00b96b' }}>{text}</span>
      ),
    },
    {
      title: '注释',
      dataIndex: 'comment',
      key: 'comment',
      ellipsis: true,
      render: (text: string) => <span style={{ color: '#aaa' }}>{text || '-'}</span>,
    },
    {
      title: '行数',
      dataIndex: 'row_count',
      key: 'row_count',
      align: 'right' as const,
      render: (count: number | undefined) => (
        <Tag
          color="default"
          style={{
            background: 'transparent',
            border: '1px solid #333',
            fontFamily: 'JetBrains Mono',
          }}
        >
          {(count ?? 0).toLocaleString()}
        </Tag>
      ),
    },
  ]

  if (!config) {
    return (
      <TechCard>
        <div style={{ textAlign: 'center', padding: 40, color: '#666' }}>
          <DatabaseOutlined style={{ fontSize: 48, marginBottom: 16, opacity: 0.5 }} />
          <p style={{ fontFamily: 'Orbitron' }}>尚未连接数据库</p>
        </div>
      </TechCard>
    )
  }

  return (
    <TechCard delay={100}>
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'flex-start',
          marginBottom: 16,
        }}
      >
        <SectionHeader title="数据表浏览" subtitle={`已发现 ${tables.length} 个对象`} />
        <Space>
          <TechButton size="small" onClick={loadTables} icon={<ReloadOutlined />} loading={loading}>
            刷新
          </TechButton>
        </Space>
      </div>

      <Space direction="vertical" style={{ width: '100%', marginBottom: 16 }}>
        <div style={{ display: 'flex', gap: 12 }}>
          <Input
            placeholder="搜索表名..."
            prefix={<SearchOutlined style={{ color: '#00b96b' }} />}
            value={searchText}
            onChange={(e) => setSearchText(e.target.value)}
            allowClear
            style={{ fontFamily: 'JetBrains Mono' }}
          />
          <TechButton size="small" onClick={handleSelectAll} icon={<CheckSquareOutlined />}>
            全选
          </TechButton>
          <TechButton size="small" onClick={handleClearAll} icon={<DeleteOutlined />} danger>
            清除
          </TechButton>
        </div>
      </Space>

      <Table
        rowKey="name"
        columns={columns}
        dataSource={filteredTables}
        loading={loading}
        rowSelection={{
          selectedRowKeys: selectedTables,
          onChange: handleRowSelection,
        }}
        pagination={{
          current: pagination.current,
          pageSize: pagination.pageSize,
          showSizeChanger: true,
          showQuickJumper: true,
          onChange: (current, pageSize) => setPagination({ current, pageSize }),
          showTotal: (total) => <span style={{ fontFamily: 'JetBrains Mono' }}>共 {total} 项</span>,
        }}
        scroll={{ y: 400 }}
        onChange={(pager) => {
          setPagination({
            current: pager.current || 1,
            pageSize: pager.pageSize || 10,
          })
        }}
        size="small"
      />
    </TechCard>
  )
}