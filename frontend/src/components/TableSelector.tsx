import { useEffect, useState } from 'react'
import { Collapse, Button, Space, Spin, Tag } from 'antd'
import { DeleteOutlined, ReloadOutlined, CodeOutlined } from '@ant-design/icons'
import { useExportStore } from '@/store/useExportStore'
import { getTableDetails } from '@/services/api'
import type { TableDetails } from '@/types'
import { TechCard } from './common/TechCard'
import { SectionHeader } from './common/SectionHeader'

const { Panel } = Collapse

export default function TableSelector() {
  const selectedTables = useExportStore((state) => state.selectedTables)
  const toggleTable = useExportStore((state) => state.toggleTable)
  const tables = useExportStore((state) => state.tables)

  const [detailsMap, setDetailsMap] = useState<Record<string, TableDetails | null>>({})
  const [loadingMap, setLoadingMap] = useState<Record<string, boolean>>({})

  const fetchDetails = async (tableName: string) => {
    const config = useExportStore.getState().connectionConfig
    if (!config) return
    setLoadingMap((prev) => ({ ...prev, [tableName]: true }))
    try {
      const res = await getTableDetails(config, tableName)
      if (res.success && res.data) {
        setDetailsMap((prev) => ({ ...prev, [tableName]: res.data as TableDetails | null }))
      } else {
        setDetailsMap((prev) => ({ ...prev, [tableName]: null }))
      }
    } catch {
      setDetailsMap((prev) => ({ ...prev, [tableName]: null }))
    } finally {
      setLoadingMap((prev) => ({ ...prev, [tableName]: false }))
    }
  }

  useEffect(() => {
    selectedTables.forEach((name) => {
      if (!detailsMap[name]) {
        fetchDetails(name)
      }
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedTables])

  if (selectedTables.length === 0) {
    return null
  }

  return (
    <TechCard delay={300} style={{ marginTop: 24 }}>
      <SectionHeader title="载荷清单" subtitle={`项目数: ${selectedTables.length}`} />
      <Collapse 
        ghost 
        expandIcon={({ isActive }) => <CodeOutlined rotate={isActive ? 90 : 0} style={{ color: '#00b96b' }} />}
      >
        {selectedTables.map(tableName => {
          const summary = tables.find(t => t.name === tableName)
          const details = detailsMap[tableName]
          const loading = loadingMap[tableName]
          const colCount = details?.columns?.length
          const pkCount = details?.primary_keys?.length

          return (
            <Panel
              header={
                <Space style={{ width: '100%', justifyContent: 'space-between' }}>
                  <span style={{ fontFamily: 'JetBrains Mono', color: '#e0e0e0' }}>{tableName}</span>
                  <Space>
                    {summary && (
                      <Tag color="rgba(255,255,255,0.1)" style={{ border: 'none', color: '#888' }}>
                        {(summary.row_count ?? 0).toLocaleString()} 行
                      </Tag>
                    )}
                    <Button
                      type="text"
                      size="small"
                      danger
                      icon={<DeleteOutlined />}
                      onClick={(e) => {
                        e.stopPropagation()
                        toggleTable(tableName)
                      }}
                    />
                  </Space>
                </Space>
              }
              key={tableName}
              extra={
                <Button
                  type="link"
                  size="small"
                  icon={<ReloadOutlined />}
                  onClick={(e) => {
                    e.stopPropagation()
                    fetchDetails(tableName)
                  }}
                >
                </Button>
              }
              style={{ borderBottom: '1px solid rgba(255,255,255,0.05)' }}
            >
              {loading && (
                <div style={{ textAlign: 'center', padding: 10 }}>
                  <Spin size="small" /> <span style={{ marginLeft: 8, fontFamily: 'JetBrains Mono', fontSize: 12 }}>分析中...</span>
                </div>
              )}
              {!loading && (
                <div style={{ fontFamily: 'JetBrains Mono', fontSize: '12px', color: '#aaa', paddingLeft: 24 }}>
                  <p>名称: <span style={{ color: '#fff' }}>{tableName}</span></p>
                  <p>行数: <span style={{ color: '#fff' }}>{(summary?.row_count ?? 0).toLocaleString()}</span></p>
                  <p>列数: <span style={{ color: '#fff' }}>{colCount ?? 'N/A'}</span></p>
                  <p>主键: <span style={{ color: '#fff' }}>{pkCount ?? 'N/A'}</span></p>
                  <div style={{ marginTop: 8, padding: 8, background: 'rgba(0,0,0,0.3)', borderLeft: '2px solid #00b96b' }}>
                    // 元数据就绪
                  </div>
                </div>
              )}
            </Panel>
          )
        })}
      </Collapse>
    </TechCard>
  )
}
