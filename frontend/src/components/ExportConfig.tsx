import { useState, useEffect } from 'react'
import { Form, Checkbox, Input, InputNumber, Space, message, Progress, Typography, Row, Col } from 'antd'
import { ClockCircleOutlined, FileTextOutlined, DatabaseOutlined, RocketOutlined } from '@ant-design/icons'
import { animate } from 'animejs'
import type { ExportRequest } from '@/types'
import { exportDDL, exportData } from '@/services/api'
import { useExportStore } from '@/store/useExportStore'
import { TechCard } from './common/TechCard'
import { TechButton } from './common/TechButton'
import { SectionHeader } from './common/SectionHeader'

const { Text } = Typography;

export default function ExportConfig() {
  const config = useExportStore((state) => state.connectionConfig)
  const selectedTables = useExportStore((state) => state.selectedTables)

  const [form] = Form.useForm()
  const [loading, setLoading] = useState(false)
  const [progress, setProgress] = useState(0)
  const [animatedProgress, setAnimatedProgress] = useState(0)
  const [elapsedTime, setElapsedTime] = useState(0)
  const [exportResult, setExportResult] = useState<{ ddl?: string; data?: string } | null>(null)

  // Animate progress bar using anime.js easing
  useEffect(() => {
    const progressObj = { value: animatedProgress }
    
    animate(progressObj, {
      value: progress,
      easing: 'easeOutExpo', // Using built-in ease as requested
      duration: 1000,
      round: 1,
      onUpdate: () => {
        setAnimatedProgress(progressObj.value)
      }
    })
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [progress])

  // Timer logic
  useEffect(() => {
    let interval: any
    if (loading) {
      const startTime = Date.now() - elapsedTime
      interval = setInterval(() => {
        setElapsedTime(Date.now() - startTime)
      }, 100)
    }
    return () => clearInterval(interval)
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loading])

  useEffect(() => {
    if (config) {
      form.setFieldsValue({
        export_schema: config.export_schema ?? config.schema,
      })
    }
  }, [config, form])

  const formatTime = (ms: number) => {
    const totalSeconds = Math.floor(ms / 1000)
    const m = Math.floor(totalSeconds / 60)
    const s = totalSeconds % 60
    return `${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}`
  }

  const handleExport = async () => {
    if (!config || selectedTables.length === 0) {
      message.warning('序列中止: 前置条件未满足')
      return
    }

    try {
      const values = await form.validateFields()
      setLoading(true)
      setProgress(0)
      setElapsedTime(0)
      setExportResult(null)

      const request: ExportRequest = {
        config,
        export_schema: values.export_schema?.trim() || undefined,
        tables: selectedTables,
        include_ddl: values.include_ddl,
        include_data: values.include_data,
        batch_size: values.batch_size || 1000,
      }

      const results: { ddl?: string; data?: string } = {}

      if (values.include_ddl) {
        setProgress(25)
        const ddlResult = await exportDDL(request)
        if (ddlResult.success && ddlResult.data) {
          results.ddl = ddlResult.data.file_path
          message.success('DDL 已提取')
        } else {
          message.error(ddlResult.error || 'DDL 提取失败')
        }
        setProgress(50)
      }

      if (values.include_data) {
        setProgress(values.include_ddl ? 50 : 25)
        const dataResult = await exportData(request)
        if (dataResult.success && dataResult.data) {
          results.data = dataResult.data.file_path
          message.success('数据已迁移')
        } else {
          message.error(dataResult.error || '数据迁移失败')
        }
        setProgress(100)
      }

      if (!values.include_ddl && !values.include_data) {
        message.warning('请选择导出模块')
        setProgress(0)
      } else {
        setExportResult(results)
      }
    } catch (error) {
      message.error('严重错误')
    } finally {
      setLoading(false)
    }
  }

  return (
    <TechCard>
      <SectionHeader title="导出控制台" subtitle="初始化数据传输序列" />
      
      <Form
        form={form}
        layout="vertical"
        initialValues={{
          include_ddl: true,
          include_data: true,
          batch_size: 1000,
        }}
      >
        <Row gutter={24}>
          <Col span={12}>
            <Form.Item name="include_ddl" valuePropName="checked">
              <Checkbox style={{ fontFamily: 'Orbitron', color: '#fff' }}>
                <FileTextOutlined style={{ marginRight: 8, color: '#00b96b' }} />
                提取 DDL (表结构)
              </Checkbox>
            </Form.Item>
          </Col>
          <Col span={12}>
            <Form.Item name="include_data" valuePropName="checked">
              <Checkbox style={{ fontFamily: 'Orbitron', color: '#fff' }}>
                 <DatabaseOutlined style={{ marginRight: 8, color: '#00b96b' }} />
                 提取数据 (INSERT)
              </Checkbox>
            </Form.Item>
          </Col>
        </Row>

        <Form.Item
          label={<span style={{ fontFamily: 'JetBrains Mono' }}>批次大小 (行/插入)</span>}
          name="batch_size"
        >
          <InputNumber 
            min={100} max={10000} step={100} 
            style={{ width: '100%', fontFamily: 'JetBrains Mono' }} 
          />
        </Form.Item>

        <Form.Item
          label={<span style={{ fontFamily: 'JetBrains Mono' }}>导出模式 (EXPORT SCHEMA)</span>}
          name="export_schema"
        >
          <Input placeholder="留空默认同 SCHEMA" style={{ width: '100%', fontFamily: 'JetBrains Mono' }} />
        </Form.Item>

        <Form.Item>
          <Space direction="vertical" style={{ width: '100%', marginTop: 24 }}>
            <TechButton
              type="primary"
              icon={<RocketOutlined />}
              onClick={handleExport}
              loading={loading}
              disabled={!config || selectedTables.length === 0}
              block
              style={{ height: '50px', fontSize: '16px', letterSpacing: '2px' }}
            >
              {loading ? '执行中...' : '启动导出'}
            </TechButton>

            {(loading || progress > 0) && (
              <div style={{ marginTop: 24, padding: 16, background: 'rgba(0,0,0,0.3)', border: '1px solid rgba(255,255,255,0.1)' }}>
                <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 8, fontFamily: 'JetBrains Mono', fontSize: 12 }}>
                  <Text type="secondary">进度状态</Text>
                  <Text type="secondary" style={{ color: '#00b96b' }}>
                     <ClockCircleOutlined style={{ marginRight: 4 }} />
                     {formatTime(elapsedTime)}
                  </Text>
                </div>
                <Progress 
                  percent={animatedProgress} 
                  status={loading ? 'active' : (progress === 100 ? 'success' : 'normal')} 
                  strokeColor={{ '0%': '#108ee9', '100%': '#87d068' }}
                  trailColor="rgba(255,255,255,0.1)"
                />
              </div>
            )}

            {exportResult && (
              <div style={{ marginTop: 24, borderLeft: '4px solid #52c41a', paddingLeft: 16, background: 'rgba(82, 196, 26, 0.1)', padding: 16 }}>
                <div style={{ fontFamily: 'Orbitron', color: '#52c41a', marginBottom: 8, fontWeight: 'bold' }}>
                  任务完成
                </div>
                <Space direction="vertical" style={{ width: '100%', fontFamily: 'JetBrains Mono', fontSize: 12 }}>
                  {exportResult.ddl && <div>&gt; DDL 文件: <span style={{ color: '#fff' }}>{exportResult.ddl}</span></div>}
                  {exportResult.data && <div>&gt; 数据文件: <span style={{ color: '#fff' }}>{exportResult.data}</span></div>}
                  <div style={{ marginTop: 8, color: '#aaa' }}>// 总耗时: {formatTime(elapsedTime)}</div>
                </Space>
              </div>
            )}
          </Space>
        </Form.Item>
      </Form>
    </TechCard>
  )
}
