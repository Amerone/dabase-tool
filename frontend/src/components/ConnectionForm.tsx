import { useCallback, useEffect, useState } from 'react'
import { Form, Input, message, Space, Row, Col, Select, Modal } from 'antd'
import {
  CheckCircleOutlined,
  CloseCircleOutlined,
  ArrowRightOutlined,
  CloudDownloadOutlined,
  SaveOutlined,
  ThunderboltOutlined,
  ExclamationCircleOutlined,
} from '@ant-design/icons'
import type { ConnectionConfig, DbType } from '@/types'
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

  const dbPortDefaults: Record<DbType, string> = {
    dm8: '5236',
    mysql: '3306',
    kingbase: '54321',
    shentong: '2003',
  }

  const handleTest = async () => {
    try {
      const values = await form.validateFields()
      setLoading(true)
      setConnectionStatus(null)

      const config: ConnectionConfig = {
        db_type: values.db_type ?? 'dm8',
        host: values.host,
        port: parseInt(values.port, 10),
        username: values.username,
        password: values.password,
        schema: values.schema,
        export_schema: normalizeExportSchema(values.export_schema),
      }

      const result = await testConnection(config)

      if (result.success && result.data?.success) {
        setConnectionStatus('success')
        message.success('连接成功')
        setConnectionConfig(
          config,
          hasUnsavedChanges ? 'manual' : loadedFrom ?? 'manual',
          null,
          true
        )
      } else {
        setConnectionStatus('error')
        const errorMsg = result.error || result.data?.message || '连接失败'
        Modal.error({
          title: '数据库连接失败',
          icon: <ExclamationCircleOutlined />,
          content: (
            <div style={{ fontFamily: 'JetBrains Mono', fontSize: 13, marginTop: 16 }}>
              <div style={{ color: '#ff4d4f', marginBottom: 8 }}>错误详情：</div>
              <div style={{
                background: 'rgba(0,0,0,0.3)',
                padding: 12,
                borderRadius: 4,
                whiteSpace: 'pre-wrap',
                wordBreak: 'break-word',
                maxHeight: 300,
                overflow: 'auto'
              }}>
                {errorMsg}
              </div>
            </div>
          ),
          okText: '确定',
          width: 600,
        })
      }
    } catch {
      setConnectionStatus('error')
      message.error('输入无效')
    } finally {
      setLoading(false)
    }
  }

  const loadSaved = useCallback(async (showMessage = true) => {
    try {
      setLoadingSaved(true)
      const requestedDbType = (form.getFieldValue('db_type') as DbType | undefined) ?? 'dm8'
      const result = await getSavedConnection(requestedDbType)
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
          message.warning('Configuration loaded. Enter password and test connection.')
        }
      } else if (showMessage) {
        message.warning(result.error || '没有保存的配置')
      }
    } catch {
      if (showMessage) {
        message.error('加载配置失败')
      }
    } finally {
      setLoadingSaved(false)
    }
  }, [form, setConnectionConfig, setLoadedFrom])

  const handleSave = async () => {
    try {
      const values = await form.validateFields()
      setSaving(true)

      const payload: ConnectionConfig = {
        db_type: values.db_type ?? 'dm8',
        host: values.host,
        port: parseInt(values.port, 10),
        username: values.username,
        password: values.password,
        schema: values.schema,
        export_schema: normalizeExportSchema(values.export_schema),
      }

      const result = await saveConnection(payload)
      if (result.success && result.data) {
        const { config, source, updated_at } = result.data

        // Backend now redacts password in responses. Keep the in-session password from form.
        const runtimeConfig: ConnectionConfig = {
          ...config,
          password: values.password,
        }

        setConnectionConfig({ ...runtimeConfig, source, updated_at }, 'saved', updated_at ?? null, false)
        setLoadedFrom('saved', updated_at ?? null)
        setHasUnsavedChanges(false)
        setConnectionStatus(null)

        if (values.password) {
          message.success('配置已保存')
        } else {
          message.warning('配置已保存，请输入密码后测试连接')
        }
      } else {
        message.error(result.error || '保存配置失败')
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
          setDriverInfo(info)
        }
      })
      .catch(() => {})
  }, [loadSaved, setDriverInfo])

  return (
    <TechCard delay={200}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
        <SectionHeader
          title="数据库连接"
          subtitle={driverInfo ? `驱动: ${driverInfo.source} // ${driverInfo.path}` : '驱动: 系统默认'}
        />
        {connectionStatus === 'success' && (
          <div style={{ color: '#52c41a', fontFamily: 'Orbitron', display: 'flex', alignItems: 'center', gap: 8 }}>
            <CheckCircleOutlined /> 已连接
          </div>
        )}
        {connectionStatus === 'error' && (
          <div style={{ color: '#ff4d4f', fontFamily: 'Orbitron', display: 'flex', alignItems: 'center', gap: 8 }}>
            <CloseCircleOutlined /> 连接失败
          </div>
        )}
      </div>

      <Form
        form={form}
        layout="vertical"
        initialValues={{
          db_type: 'dm8',
          host: 'localhost',
          port: '5236',
          username: '',
          password: '',
          schema: '',
          export_schema: '',
        }}
        onValuesChange={(changedValues) => {
          // Auto-populate schema from username when schema is empty
          if (changedValues.username !== undefined) {
            const currentSchema = form.getFieldValue('schema')
            if (!currentSchema) {
              form.setFieldValue('schema', changedValues.username)
            }
          }
          setHasUnsavedChanges(true)
          setLoadedFrom('manual', lastUpdatedAt)
          setConnectionStatus(null)
        }}
      >
        <Form.Item
          label={<span style={{ fontFamily: 'JetBrains Mono' }}>数据库类型</span>}
          name="db_type"
          rules={[{ required: true, message: '请选择数据库类型' }]}
        >
          <Select
            style={{ fontFamily: 'JetBrains Mono' }}
            options={[
              { label: 'DM8', value: 'dm8' },
              { label: 'MySQL', value: 'mysql' },
              { label: 'KingbaseES', value: 'kingbase' },
              { label: 'ShenTong/OSCAR', value: 'shentong' },
            ]}
            onChange={(value: DbType) => {
              const currentPort = form.getFieldValue('port')
              if (
                !currentPort ||
                currentPort === dbPortDefaults.dm8 ||
                currentPort === dbPortDefaults.mysql ||
                currentPort === dbPortDefaults.kingbase ||
                currentPort === dbPortDefaults.shentong
              ) {
                form.setFieldValue('port', dbPortDefaults[value])
              }
            }}
          />
        </Form.Item>

        <Row gutter={24}>
          <Col span={16}>
            <Form.Item
              label={<span style={{ fontFamily: 'JetBrains Mono' }}>主机</span>}
              name="host"
              rules={[{ required: true, message: '请输入主机地址' }]}
            >
              <Input placeholder="localhost" style={{ fontFamily: 'JetBrains Mono' }} />
            </Form.Item>
          </Col>
          <Col span={8}>
            <Form.Item
              label={<span style={{ fontFamily: 'JetBrains Mono' }}>端口</span>}
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
              label={<span style={{ fontFamily: 'JetBrains Mono' }}>用户名</span>}
              name="username"
              rules={[{ required: true, message: '请输入用户名' }]}
            >
              <Input placeholder="SYSDBA" style={{ fontFamily: 'JetBrains Mono' }} />
            </Form.Item>
          </Col>
          <Col span={12}>
            <Form.Item
              label={<span style={{ fontFamily: 'JetBrains Mono' }}>密码</span>}
              name="password"
              rules={[{ required: true, message: '请输入密码' }]}
            >
              <Input.Password placeholder="******" style={{ fontFamily: 'JetBrains Mono' }} />
            </Form.Item>
          </Col>
        </Row>

        <Form.Item
          label={<span style={{ fontFamily: 'JetBrains Mono' }}>模式名</span>}
          name="schema"
          rules={[{ required: true, message: '请输入模式名' }]}
        >
          <Input placeholder="与用户名相同（如 SYSDBA / PLATFORM）" style={{ fontFamily: 'JetBrains Mono' }} />
        </Form.Item>

        <Form.Item
          label={<span style={{ fontFamily: 'JetBrains Mono' }}>导出模式名</span>}
          name="export_schema"
        >
          <Input placeholder="可选，默认与模式名相同" style={{ fontFamily: 'JetBrains Mono' }} />
        </Form.Item>

        <div
          style={{
            marginTop: 32,
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center',
            borderTop: '1px solid rgba(255,255,255,0.1)',
            paddingTop: 24,
          }}
        >
          <Space>
            <TechButton onClick={() => loadSaved(true)} loading={loadingSaved} icon={<CloudDownloadOutlined />}>
              加载配置
            </TechButton>
            <TechButton onClick={handleSave} loading={saving} icon={<SaveOutlined />}>
              保存配置
            </TechButton>
          </Space>

          <Space>
            <TechButton
              onClick={handleTest}
              loading={loading}
              icon={<ThunderboltOutlined />}
              type="default"
              style={{ borderColor: '#1890ff', color: '#1890ff' }}
            >
              测试连接
            </TechButton>
            <TechButton
              type="primary"
              onClick={handleNext}
              disabled={connectionStatus !== 'success'}
              icon={<ArrowRightOutlined />}
              style={{ minWidth: 140 }}
            >
              继续
            </TechButton>
          </Space>
        </div>

        {hasUnsavedChanges && loadedFrom !== 'saved' && (
          <div style={{ marginTop: 12, color: '#faad14', fontFamily: 'JetBrains Mono', fontSize: 12 }}>
            有未保存的修改
          </div>
        )}
      </Form>
    </TechCard>
  )
}
