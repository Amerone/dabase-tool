import { useEffect, useState } from 'react'
import { Form, Input, message, Space, Row, Col } from 'antd'
import { CheckCircleOutlined, CloseCircleOutlined, ArrowRightOutlined, CloudDownloadOutlined, SaveOutlined, ThunderboltOutlined } from '@ant-design/icons'
import type { ConnectionConfig, DriverInfo } from '@/types'
import { testConnection, getSavedConnection, saveConnection, getDriverInfo } from '@/services/api'
import { useExportStore } from '@/store/useExportStore'
import { TechCard } from './common/TechCard'
import { TechButton } from './common/TechButton'
import { SectionHeader } from './common/SectionHeader'

export default function ConnectionForm() {
  const [form] = Form.useForm()
  const [loading, setLoading] = useState(false)
  const [loadingSaved, setLoadingSaved] = useState(false)
  const [saving, setSaving] = useState(false)
  const [hasUnsavedChanges, setHasUnsavedChanges] = useState(false)
  const [connectionStatus, setConnectionStatus] = useState<'success' | 'error' | null>(null)
  
  const setConnectionConfig = useExportStore((state) => state.setConnectionConfig)
  const nextStep = useExportStore((state) => state.nextStep)
  const loadedFrom = useExportStore((state) => state.loadedFrom)
  const lastUpdatedAt = useExportStore((state) => state.lastUpdatedAt)
  const setLoadedFrom = useExportStore((state) => state.setLoadedFrom)
  const driverInfo = useExportStore((state) => state.driverInfo)
  const setDriverInfo = useExportStore((state) => state.setDriverInfo)

  const normalizeExportSchema = (value?: string) => {
    const trimmed = value?.trim()
    return trimmed ? trimmed : undefined
  }

  const handleTest = async () => {
    try {
      const values = await form.validateFields()
      setLoading(true)
      setConnectionStatus(null)

      const config: ConnectionConfig = {
        host: values.host,
        port: parseInt(values.port),
        username: values.username,
        password: values.password,
        schema: values.schema,
        export_schema: normalizeExportSchema(values.export_schema),
      }

      const result = await testConnection(config)

      if (result.success && result.data?.success) {
        setConnectionStatus('success')
        message.success('系统已连接')
        setConnectionConfig(config, hasUnsavedChanges ? 'manual' : loadedFrom ?? 'manual', null, true)
      } else {
        setConnectionStatus('error')
        message.error(result.error || result.data?.message || '连接失败')
      }
    } catch {
      setConnectionStatus('error')
      message.error('输入无效')
    } finally {
      setLoading(false)
    }
  }

  const loadSaved = async (showMessage = true) => {
    try {
      setLoadingSaved(true)
      const result = await getSavedConnection()
      if (result.success && result.data) {
        const { config, source, updated_at } = result.data
        form.setFieldsValue({
          ...config,
          port: config.port?.toString(),
        })
        setConnectionConfig(
          { ...config, source, updated_at },
          source === 'sqlite' ? 'saved' : 'manual',
          updated_at ?? null,
          false
        )
        setLoadedFrom(source === 'sqlite' ? 'saved' : 'manual', updated_at ?? null)
        setConnectionStatus(null)
        setHasUnsavedChanges(false)
        if (showMessage) {
          message.success('配置已加载')
        }
      } else if (showMessage) {
        message.warning(result.error || '无保存配置')
      }
    } catch {
      if (showMessage) {
        message.error('加载失败')
      }
    } finally {
      setLoadingSaved(false)
    }
  }

  const handleSave = async () => {
    try {
      const values = await form.validateFields()
      setSaving(true)
      const payload: ConnectionConfig = {
        host: values.host,
        port: parseInt(values.port),
        username: values.username,
        password: values.password,
        schema: values.schema,
        export_schema: normalizeExportSchema(values.export_schema),
      }
      const result = await saveConnection(payload)
      if (result.success && result.data) {
        const { config, source, updated_at } = result.data
        setConnectionConfig(
          { ...config, source, updated_at },
          'saved',
          updated_at ?? null,
          false
        )
        setLoadedFrom('saved', updated_at ?? null)
        setHasUnsavedChanges(false)
        setConnectionStatus(null)
        message.success('配置已保存')
      } else {
        message.error(result.error || '保存失败')
      }
    } catch {
      message.error('请填写必填项')
    } finally {
      setSaving(false)
    }
  }

  const handleNext = async () => {
    if (connectionStatus === 'success') {
      nextStep()
    } else {
      await handleTest()
    }
  }

  useEffect(() => {
    loadSaved(false)
    getDriverInfo()
      .then((info) => {
        if (info) {
          setDriverInfo(info as DriverInfo)
        }
      })
      .catch(() => {})
  }, [])

  return (
    <TechCard delay={200}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
        <SectionHeader 
          title="数据库连接链路" 
          subtitle={driverInfo ? `驱动: ${driverInfo.source} // ${driverInfo.path}` : '驱动: 系统默认'} 
        />
        {connectionStatus === 'success' && (
          <div style={{ color: '#52c41a', fontFamily: 'Orbitron', display: 'flex', alignItems: 'center', gap: 8 }}>
            <CheckCircleOutlined /> 链路已建立
          </div>
        )}
        {connectionStatus === 'error' && (
          <div style={{ color: '#ff4d4f', fontFamily: 'Orbitron', display: 'flex', alignItems: 'center', gap: 8 }}>
            <CloseCircleOutlined /> 链路连接失败
          </div>
        )}
      </div>

      <Form
        form={form}
        layout="vertical"
        initialValues={{
          host: 'localhost',
          port: '5236',
          username: 'SYSDBA',
          password: '',
          schema: 'SYSDBA',
          export_schema: '',
        }}
        onValuesChange={() => {
          setHasUnsavedChanges(true)
          setLoadedFrom('manual', lastUpdatedAt)
          setConnectionStatus(null)
        }}
      >
        <Row gutter={24}>
          <Col span={16}>
            <Form.Item
              label={<span style={{ fontFamily: 'JetBrains Mono' }}>主机地址 (HOST)</span>}
              name="host"
              rules={[{ required: true, message: '请输入主机地址' }]}
            >
              <Input placeholder="localhost" style={{ fontFamily: 'JetBrains Mono' }} />
            </Form.Item>
          </Col>
          <Col span={8}>
            <Form.Item
              label={<span style={{ fontFamily: 'JetBrains Mono' }}>端口 (PORT)</span>}
              name="port"
              rules={[{ required: true, message: '请输入端口' }]}
            >
              <Input placeholder="5236" style={{ fontFamily: 'JetBrains Mono' }} />
            </Form.Item>
          </Col>
        </Row>

        <Row gutter={24}>
          <Col span={12}>
            <Form.Item
              label={<span style={{ fontFamily: 'JetBrains Mono' }}>用户标识 (USER)</span>}
              name="username"
              rules={[{ required: true, message: '请输入用户名' }]}
            >
              <Input placeholder="SYSDBA" style={{ fontFamily: 'JetBrains Mono' }} />
            </Form.Item>
          </Col>
          <Col span={12}>
            <Form.Item
              label={<span style={{ fontFamily: 'JetBrains Mono' }}>访问密钥 (KEY)</span>}
              name="password"
              rules={[{ required: true, message: '请输入密码' }]}
            >
              <Input.Password placeholder="••••••" style={{ fontFamily: 'JetBrains Mono' }} />
            </Form.Item>
          </Col>
        </Row>

        <Form.Item
          label={<span style={{ fontFamily: 'JetBrains Mono' }}>目标模式 (SCHEMA)</span>}
          name="schema"
          rules={[{ required: true, message: '请输入模式名' }]}
        >
          <Input placeholder="SYSDBA" style={{ fontFamily: 'JetBrains Mono' }} />
        </Form.Item>

        <Form.Item
          label={<span style={{ fontFamily: 'JetBrains Mono' }}>导出模式 (EXPORT SCHEMA)</span>}
          name="export_schema"
        >
          <Input placeholder="留空默认同 SCHEMA" style={{ fontFamily: 'JetBrains Mono' }} />
        </Form.Item>

        <div style={{ marginTop: 32, display: 'flex', justifyContent: 'space-between', alignItems: 'center', borderTop: '1px solid rgba(255,255,255,0.1)', paddingTop: 24 }}>
          <Space>
            <TechButton onClick={() => loadSaved(true)} loading={loadingSaved} icon={<CloudDownloadOutlined />}>
              加载配置
            </TechButton>
            <TechButton 
              onClick={handleSave} 
              loading={saving}
              icon={<SaveOutlined />}
            >
              保存配置
            </TechButton>
          </Space>

          <Space>
            <TechButton onClick={handleTest} loading={loading} icon={<ThunderboltOutlined />} type="default" style={{ borderColor: '#1890ff', color: '#1890ff' }}>
              测试链路
            </TechButton>
            <TechButton 
              type="primary" 
              onClick={handleNext} 
              disabled={connectionStatus !== 'success'}
              icon={<ArrowRightOutlined />}
              style={{ minWidth: 140 }}
            >
              初始化连接
            </TechButton>
          </Space>
        </div>
        
        {hasUnsavedChanges && loadedFrom !== 'saved' && (
           <div style={{ marginTop: 12, color: '#faad14', fontFamily: 'JetBrains Mono', fontSize: 12 }}>
             ⚠ 检测到未保存的更改
           </div>
        )}
      </Form>
    </TechCard>
  )
}
