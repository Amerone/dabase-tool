import { useState, useEffect, useRef } from 'react'
import {
  Form,
  Checkbox,
  Input,
  InputNumber,
  Space,
  message,
  Progress,
  Typography,
  Row,
  Col,
  Select,
} from 'antd'
import {
  ClockCircleOutlined,
  FileTextOutlined,
  DatabaseOutlined,
  RocketOutlined,
  FolderOpenOutlined,
  CopyOutlined,
} from '@ant-design/icons'
import { animate } from 'animejs'
import type { CapabilityLevel, ExportRequest } from '@/types'
import { exportDDL, exportData, getExportCapabilities, getExportDirectory } from '@/services/api'
import { calcProgress } from '@/utils/exportProgress'
import { useExportStore } from '@/store/useExportStore'
import { TechCard } from './common/TechCard'
import { TechButton } from './common/TechButton'
import { SectionHeader } from './common/SectionHeader'

const { Text } = Typography

const capabilityRank: Record<CapabilityLevel, number> = {
  none: 0,
  partial: 1,
  full: 2,
}

const isSupportedLevel = (level?: CapabilityLevel) =>
  (level ? capabilityRank[level] : 0) > capabilityRank.none

type ExportOutcome = 'success' | 'partial' | 'failed' | null

export default function ExportConfig() {
  const config = useExportStore((state) => state.connectionConfig)
  const selectedTables = useExportStore((state) => state.selectedTables)

  const [form] = Form.useForm()
  const watchedTargetDialect = Form.useWatch('target_dialect', form)
  const [loading, setLoading] = useState(false)
  const [progress, setProgress] = useState(0)
  const [animatedProgress, setAnimatedProgress] = useState(0)
  const [progressStatus, setProgressStatus] = useState<'normal' | 'active' | 'success' | 'exception'>(
    'normal'
  )
  const [elapsedTime, setElapsedTime] = useState(0)
  const [exportResult, setExportResult] = useState<{ ddl?: string; data?: string } | null>(null)
  const [exportOutcome, setExportOutcome] = useState<ExportOutcome>(null)
  const [includeRowCountsSupported, setIncludeRowCountsSupported] = useState(true)
  const [includeRowCountsNote, setIncludeRowCountsNote] = useState<string | null>(null)
  const resultRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const progressObj = { value: animatedProgress }

    animate(progressObj, {
      value: progress,
      easing: 'easeOutExpo',
      duration: 1000,
      round: 1,
      onUpdate: () => {
        setAnimatedProgress(progressObj.value)
      },
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [progress])

  useEffect(() => {
    let interval: ReturnType<typeof setInterval> | undefined
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
        target_dialect: config.db_type ?? 'dm8',
      })
    }
  }, [config, form])

  // Auto-scroll to result when export finishes
  useEffect(() => {
    if (exportResult && resultRef.current) {
      resultRef.current.scrollIntoView({ behavior: 'smooth', block: 'start' })
    }
  }, [exportResult])

  useEffect(() => {
    if (!config) {
      setIncludeRowCountsSupported(false)
      setIncludeRowCountsNote(null)
      return
    }

    const sourceDbType = config.db_type ?? 'dm8'
    const targetDialect = watchedTargetDialect ?? sourceDbType
    let cancelled = false

    const loadCapability = async () => {
      const capabilityResp = await getExportCapabilities(sourceDbType, targetDialect)
      if (cancelled) {
        return
      }

      if (capabilityResp.success && capabilityResp.data) {
        const supported = capabilityResp.data.data_options?.include_row_counts_supported ?? false
        const note = capabilityResp.data.data_options?.include_row_counts_note ?? null
        setIncludeRowCountsSupported(supported)
        setIncludeRowCountsNote(note)
        if (!supported) {
          form.setFieldValue('include_row_counts', false)
        }
      } else {
        setIncludeRowCountsSupported(false)
        setIncludeRowCountsNote(
          capabilityResp.error || '当前源/目标组合不支持行数预扫描'
        )
        form.setFieldValue('include_row_counts', false)
      }
    }

    loadCapability().catch(() => {
      if (!cancelled) {
        setIncludeRowCountsSupported(false)
        setIncludeRowCountsNote('行数预扫描能力检查失败')
        form.setFieldValue('include_row_counts', false)
      }
    })

    return () => {
      cancelled = true
    }
  }, [config, watchedTargetDialect, form])

  const formatTime = (ms: number) => {
    const totalSeconds = Math.floor(ms / 1000)
    const m = Math.floor(totalSeconds / 60)
    const s = totalSeconds % 60
    return `${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}`
  }

  const isTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

  const handleOpenFolder = async () => {
    const res = await getExportDirectory()
    if (!res.success || !res.data) {
      message.error(res.error || '获取导出目录失败')
      return
    }
    const dir = res.data
    if (isTauri()) {
      try {
        const { openPath } = await import('@tauri-apps/plugin-opener')
        await openPath(dir)
      } catch {
        message.error('无法打开目录')
      }
    } else {
      try {
        await navigator.clipboard.writeText(dir)
        message.success(`路径已复制: ${dir}`)
      } catch {
        message.info(`导出目录: ${dir}`)
      }
    }
  }

  const handleExport = async () => {
    if (!config || selectedTables.length === 0) {
      message.warning('导出中止：前置条件未满足')
      return
    }
    try {
      const values = await form.validateFields()
      const sourceDbType = config.db_type ?? 'dm8'
      const targetDialect = values.target_dialect ?? sourceDbType
      const capabilityResp = await getExportCapabilities(sourceDbType, targetDialect)
      if (!capabilityResp.success || !capabilityResp.data) {
        message.error(capabilityResp.error || '加载导出能力失败')
        return
      }

      const ddlCapability = capabilityResp.data.entries.find((entry) => entry.object === 'ddl')
      const dataCapability = capabilityResp.data.entries.find((entry) => entry.object === 'data')

      if (values.include_ddl && !isSupportedLevel(ddlCapability?.effective_level)) {
        const reason = ddlCapability?.reason_code ? ` [${ddlCapability.reason_code}]` : ''
        message.warning(`${ddlCapability?.note || '当前组合不支持 DDL 导出'}${reason}`)
        return
      }
      if (values.include_data && !isSupportedLevel(dataCapability?.effective_level)) {
        const reason = dataCapability?.reason_code ? ` [${dataCapability.reason_code}]` : ''
        message.warning(`${dataCapability?.note || '当前组合不支持数据导出'}${reason}`)
        return
      }

      setLoading(true)
      setProgress(0)
      setProgressStatus('active')
      setElapsedTime(0)
      setExportResult(null)
      setExportOutcome(null)

      const request: ExportRequest = {
        config,
        target_dialect: targetDialect,
        export_schema: values.export_schema?.trim() || undefined,
        export_compat: values.export_compat,
        tables: selectedTables.map(name => ({
          schema: config.schema,
          name,
        })),
        include_ddl: values.include_ddl,
        include_data: values.include_data,
        batch_size: values.batch_size || 1000,
        drop_existing: values.drop_existing,
        include_row_counts: includeRowCountsSupported ? Boolean(values.include_row_counts) : false,
        strict_mode: Boolean(values.strict_mode),
        identifier_case: (targetDialect === 'kingbase' || targetDialect === 'shentong') ? (values.identifier_case || 'lower') : undefined,
      }

      const results: { ddl?: string; data?: string } = {}
      let completedSteps = 0
      let failedSteps = 0
      const totalSteps = (values.include_ddl ? 1 : 0) + (values.include_data ? 1 : 0)

      const updateStepProgress = () => {
        const hasAnyError = failedSteps > 0
        const nextProgress = calcProgress({
          includeDdl: values.include_ddl,
          includeData: values.include_data,
          completedSteps,
          hasError: hasAnyError,
        })
        setProgress(nextProgress.percent)
        setProgressStatus(hasAnyError ? 'exception' : nextProgress.status)
      }

      if (values.include_ddl) {
        const ddlResult = await exportDDL(request)
        if (ddlResult.success && ddlResult.data) {
          results.ddl = ddlResult.data.file_path
          message.success('DDL 导出成功')
          completedSteps += 1
        } else {
          message.error(ddlResult.error || 'DDL 导出失败')
          failedSteps += 1
        }
        updateStepProgress()
      }

      if (values.include_data) {
        const dataResult = await exportData(request)
        if (dataResult.success && dataResult.data) {
          results.data = dataResult.data.file_path
          message.success('数据导出成功')
          completedSteps += 1
        } else {
          message.error(dataResult.error || '数据导出失败')
          failedSteps += 1
        }
        updateStepProgress()
      }

      if (!values.include_ddl && !values.include_data) {
        message.warning('请至少选择一个导出模块')
        setProgress(0)
        setProgressStatus('normal')
      } else {
        if (failedSteps === 0) {
          setExportOutcome('success')
          if (results.ddl || results.data) {
            setExportResult(results)
          }
        } else if (failedSteps < totalSteps) {
          setExportOutcome('partial')
          setProgressStatus('exception')
          message.warning('导出部分成功，请检查失败步骤后重试')
          if (results.ddl || results.data) {
            setExportResult(results)
          }
        } else {
          setExportOutcome('failed')
          setProgressStatus('exception')
        }
      }
    } catch {
      message.error('导出时发生意外错误')
      const errorProgress = calcProgress({
        includeDdl: true,
        includeData: true,
        completedSteps: 0,
        hasError: true,
      })
      setProgress(errorProgress.percent)
      setProgressStatus(errorProgress.status)
      setExportOutcome('failed')
    } finally {
      setLoading(false)
    }
  }

  return (
    <TechCard>
      <SectionHeader title="导出控制面板" subtitle="配置并执行导出任务" />

      <Form
        form={form}
        layout="vertical"
        initialValues={{
          include_ddl: true,
          include_data: true,
          target_dialect: 'dm8',
          batch_size: 1000,
          drop_existing: true,
          include_row_counts: false,
          strict_mode: false,
        }}
      >
        <Row gutter={24}>
          <Col span={12}>
            <Form.Item name="include_ddl" valuePropName="checked">
              <Checkbox style={{ fontFamily: 'Orbitron', color: '#fff' }}>
                <FileTextOutlined style={{ marginRight: 8, color: '#00b96b' }} />
                导出 DDL（表结构）
              </Checkbox>
            </Form.Item>
          </Col>
          <Col span={12}>
            <Form.Item name="include_data" valuePropName="checked">
              <Checkbox style={{ fontFamily: 'Orbitron', color: '#fff' }}>
                <DatabaseOutlined style={{ marginRight: 8, color: '#00b96b' }} />
                导出数据（INSERT）
              </Checkbox>
            </Form.Item>
          </Col>
        </Row>

        <Row gutter={24}>
          <Col span={12}>
            <Form.Item name="drop_existing" valuePropName="checked">
              <Checkbox style={{ fontFamily: 'Orbitron', color: '#fff' }}>
                生成 DROP TABLE IF EXISTS
              </Checkbox>
            </Form.Item>
          </Col>
          <Col span={12}>
            <Form.Item name="include_row_counts" valuePropName="checked">
              <Checkbox style={{ fontFamily: 'Orbitron', color: '#fff' }} disabled={!includeRowCountsSupported}>
                写入行数预扫描注释
              </Checkbox>
            </Form.Item>
            {!includeRowCountsSupported && includeRowCountsNote && (
              <Text type="secondary" style={{ fontSize: 12 }}>
                {includeRowCountsNote}
              </Text>
            )}
          </Col>
        </Row>

        <Row gutter={24}>
          <Col span={12}>
            <Form.Item name="strict_mode" valuePropName="checked">
              <Checkbox style={{ fontFamily: 'Orbitron', color: '#fff' }}>
                严格模式（部分支持时快速失败）
              </Checkbox>
            </Form.Item>
          </Col>
        </Row>

        <Form.Item
          label={<span style={{ fontFamily: 'JetBrains Mono' }}>批次大小（每次插入行数）</span>}
          name="batch_size"
        >
          <InputNumber min={100} max={10000} step={100} style={{ width: '100%', fontFamily: 'JetBrains Mono' }} />
        </Form.Item>

        <Form.Item
          label={<span style={{ fontFamily: 'JetBrains Mono' }}>目标方言</span>}
          name="target_dialect"
          rules={[{ required: true, message: '请选择目标方言' }]}
        >
          <Select
            style={{ width: '100%', fontFamily: 'JetBrains Mono' }}
            options={[
              { value: 'dm8', label: 'DM8' },
              { value: 'mysql', label: 'MySQL' },
              { value: 'kingbase', label: 'KingbaseES' },
              { value: 'shentong', label: 'ShenTong/OSCAR' },
            ]}
          />
        </Form.Item>

        <Form.Item
          label={<span style={{ fontFamily: 'JetBrains Mono' }}>导出兼容模式</span>}
          name="export_compat"
          rules={[{ required: true, message: '请选择兼容模式' }]}
        >
          <Select
            placeholder="选择兼容模式"
            style={{ width: '100%', fontFamily: 'JetBrains Mono' }}
            options={[
              { value: 'datagrip', label: 'DataGrip 逐语句模式（END; 无 /）' },
              {
                value: 'datagrip-script',
                label: 'DataGrip 脚本模式（触发器单独输出）',
              },
              { value: 'script', label: 'DBeaver/SQLark/DIsql 模式（END; + /）' },
            ]}
          />
        </Form.Item>

        {(watchedTargetDialect === 'kingbase' || watchedTargetDialect === 'shentong') && (
          <Form.Item
            label={<span style={{ fontFamily: 'JetBrains Mono' }}>标识符大小写</span>}
            name="identifier_case"
            initialValue="lower"
          >
            <Select
              style={{ width: '100%', fontFamily: 'JetBrains Mono' }}
              options={[
                { value: 'lower', label: watchedTargetDialect === 'kingbase' ? '小写（Kingbase 推荐）' : '小写' },
                { value: 'upper', label: '大写' },
                { value: 'preserve', label: '保持原样' },
              ]}
            />
          </Form.Item>
        )}

        <Form.Item
          label={<span style={{ fontFamily: 'JetBrains Mono' }}>导出模式名</span>}
          name="export_schema"
        >
          <Input
            placeholder="可选，默认使用源模式名"
            style={{ width: '100%', fontFamily: 'JetBrains Mono' }}
          />
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
              {loading ? '导出中...' : '开始导出'}
            </TechButton>

            {(loading || progress > 0) && (
              <div
                style={{
                  marginTop: 24,
                  padding: 16,
                  background: 'rgba(0,0,0,0.3)',
                  border: '1px solid rgba(255,255,255,0.1)',
                }}
              >
                <div
                  style={{
                    display: 'flex',
                    justifyContent: 'space-between',
                    marginBottom: 8,
                    fontFamily: 'JetBrains Mono',
                    fontSize: 12,
                  }}
                >
                  <Text type="secondary">进度</Text>
                  <Text type="secondary" style={{ color: '#00b96b' }}>
                    <ClockCircleOutlined style={{ marginRight: 4 }} />
                    {formatTime(elapsedTime)}
                  </Text>
                </div>
                <Progress
                  percent={animatedProgress}
                  status={loading ? 'active' : progressStatus}
                  strokeColor={{ '0%': '#108ee9', '100%': '#87d068' }}
                  trailColor="rgba(255,255,255,0.1)"
                />
              </div>
            )}

            {exportResult && (
              <div
                ref={resultRef}
                style={{
                  marginTop: 24,
                  borderLeft: exportOutcome === 'partial' ? '4px solid #faad14' : '4px solid #52c41a',
                  paddingLeft: 16,
                  background:
                    exportOutcome === 'partial'
                      ? 'rgba(250, 173, 20, 0.12)'
                      : 'rgba(82, 196, 26, 0.1)',
                  padding: 16,
                }}
              >
                <div
                  style={{
                    fontFamily: 'Orbitron',
                    color: exportOutcome === 'partial' ? '#faad14' : '#52c41a',
                    marginBottom: 12,
                    fontWeight: 'bold',
                  }}
                >
                  {exportOutcome === 'partial' ? '导出部分成功' : '导出完成'}
                </div>
                <Space
                  direction="vertical"
                  style={{ width: '100%', fontFamily: 'JetBrains Mono', fontSize: 12 }}
                >
                  {exportOutcome === 'partial' && (
                    <div style={{ color: '#faad14' }}>存在失败步骤，请查看上方错误提示后重试。</div>
                  )}
                  {exportResult.ddl && (
                    <div style={{ display: 'flex', alignItems: 'flex-start', gap: 8 }}>
                      <span style={{ color: '#aaa', whiteSpace: 'nowrap' }}>&gt; DDL 文件:</span>
                      <span
                        style={{
                          color: '#fff',
                          wordBreak: 'break-all',
                          flex: 1,
                          lineHeight: 1.6,
                        }}
                      >
                        {exportResult.ddl}
                      </span>
                      <CopyOutlined
                        style={{ color: '#00b96b', cursor: 'pointer', flexShrink: 0, marginTop: 2 }}
                        title="复制路径"
                        onClick={() => {
                          navigator.clipboard.writeText(exportResult.ddl!).then(() =>
                            message.success('DDL 路径已复制')
                          )
                        }}
                      />
                    </div>
                  )}
                  {exportResult.data && (
                    <div style={{ display: 'flex', alignItems: 'flex-start', gap: 8 }}>
                      <span style={{ color: '#aaa', whiteSpace: 'nowrap' }}>&gt; 数据文件:</span>
                      <span
                        style={{
                          color: '#fff',
                          wordBreak: 'break-all',
                          flex: 1,
                          lineHeight: 1.6,
                        }}
                      >
                        {exportResult.data}
                      </span>
                      <CopyOutlined
                        style={{ color: '#00b96b', cursor: 'pointer', flexShrink: 0, marginTop: 2 }}
                        title="复制路径"
                        onClick={() => {
                          navigator.clipboard.writeText(exportResult.data!).then(() =>
                            message.success('数据路径已复制')
                          )
                        }}
                      />
                    </div>
                  )}
                  <div style={{ marginTop: 8, color: '#aaa' }}>// 耗时: {formatTime(elapsedTime)}</div>
                  <TechButton icon={<FolderOpenOutlined />} onClick={handleOpenFolder} style={{ marginTop: 12 }}>
                    打开导出目录
                  </TechButton>
                </Space>
              </div>
            )}
          </Space>
        </Form.Item>
      </Form>
    </TechCard>
  )
}
