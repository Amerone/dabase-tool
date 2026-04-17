import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  ArrowRightOutlined,
  CheckCircleOutlined,
  CloudDownloadOutlined,
  CloseCircleOutlined,
  ExclamationCircleOutlined,
  SaveOutlined,
  ThunderboltOutlined,
} from '@ant-design/icons';
import { Col, Form, Input, Modal, Row, Select, Space, message } from 'antd';

import type { ConnectionConfig, DbType } from '@/types';
import { getDriverInfo, getSavedConnection, saveConnection, testConnection } from '@/services/api';
import { useExportStore } from '@/store/useExportStore';
import { SectionHeader } from './common/SectionHeader';
import { TechButton } from './common/TechButton';
import { TechCard } from './common/TechCard';

const dbPortDefaults: Record<DbType, string> = {
  dm8: '5236',
  mysql: '3306',
  kingbase: '54321',
  shentong: '2003',
};

const dbTypeOptions: { label: string; value: DbType }[] = [
  { label: 'DM8', value: 'dm8' },
  { label: 'MySQL', value: 'mysql' },
  { label: 'KingbaseES', value: 'kingbase' },
  { label: 'ShenTong/OSCAR', value: 'shentong' },
];

function normalizeExportSchema(value?: string) {
  const trimmed = value?.trim();
  return trimmed ? trimmed : undefined;
}

export default function ConnectionForm() {
  const [form] = Form.useForm();
  const [loading, setLoading] = useState(false);
  const [loadingSaved, setLoadingSaved] = useState(false);
  const [saving, setSaving] = useState(false);
  const [hasUnsavedChanges, setHasUnsavedChanges] = useState(false);
  const [connectionStatus, setConnectionStatus] = useState<'success' | 'error' | null>(null);
  const initializedRef = useRef(false);

  const watchedDbType = Form.useWatch('db_type', form);

  const setConnectionConfig = useExportStore((state) => state.setConnectionConfig);
  const nextStep = useExportStore((state) => state.nextStep);
  const loadedFrom = useExportStore((state) => state.loadedFrom);
  const lastUpdatedAt = useExportStore((state) => state.lastUpdatedAt);
  const setLoadedFrom = useExportStore((state) => state.setLoadedFrom);
  const driverInfo = useExportStore((state) => state.driverInfo);
  const setDriverInfo = useExportStore((state) => state.setDriverInfo);

  const driverSummary = useMemo(() => {
    if (!driverInfo) {
      return '驱动来源: 系统默认';
    }

    const drivers = driverInfo.drivers ?? [];
    const optionalCount = drivers.filter((driver) => !driver.required).length;
    const loadedCount = drivers.length || 1;
    return `驱动来源: ${loadedCount} 个已加载 · 可选驱动 ${optionalCount} 个 · DM8 ${driverInfo.source}`;
  }, [driverInfo]);

  const handleTest = async () => {
    try {
      const values = await form.validateFields();
      setLoading(true);
      setConnectionStatus(null);

      const config: ConnectionConfig = {
        db_type: values.db_type ?? 'dm8',
        host: values.host,
        port: Number.parseInt(values.port, 10),
        username: values.username,
        password: values.password,
        schema: values.schema,
        export_schema: normalizeExportSchema(values.export_schema),
        database: values.database || undefined,
      };

      const result = await testConnection(config);
      if (result.success && result.data?.success) {
        setConnectionStatus('success');
        message.success('连接成功');
        setConnectionConfig(config, hasUnsavedChanges ? 'manual' : loadedFrom ?? 'manual', null, true);
        return;
      }

      setConnectionStatus('error');
      const errorMessage = result.error || result.data?.message || '连接失败';
      Modal.error({
        title: '数据库连接失败',
        icon: <ExclamationCircleOutlined />,
        width: 640,
        content: (
          <div className="connection-error-modal">
            <p>错误详情</p>
            <pre>{errorMessage}</pre>
          </div>
        ),
      });
    } catch {
      setConnectionStatus('error');
      message.error('请输入完整且合法的连接参数');
    } finally {
      setLoading(false);
    }
  };

  const loadSaved = useCallback(
    async (showMessage = true) => {
      try {
        setLoadingSaved(true);
        const requestedDbType = (form.getFieldValue('db_type') as DbType | undefined) ?? 'dm8';
        const result = await getSavedConnection(requestedDbType);

        if (result.success && result.data) {
          const { config, source, updated_at } = result.data;
          form.setFieldsValue({ ...config, port: config.port?.toString() });
          setConnectionConfig(
            { ...config, source, updated_at },
            source === 'sqlite' ? 'saved' : 'manual',
            updated_at ?? null,
            false
          );
          setLoadedFrom(source === 'sqlite' ? 'saved' : 'manual', updated_at ?? null);
          setConnectionStatus(null);
          setHasUnsavedChanges(false);
          if (showMessage) {
            message.info('已加载保存配置，请输入密码并测试连接');
          }
          return;
        }

        if (showMessage) {
          message.warning(result.error || '没有找到可用的保存配置');
        }
      } catch {
        if (showMessage) {
          message.error('加载保存配置失败');
        }
      } finally {
        setLoadingSaved(false);
      }
    },
    [form, setConnectionConfig, setLoadedFrom]
  );

  const handleSave = async () => {
    try {
      const values = await form.validateFields();
      setSaving(true);

      const payload: ConnectionConfig = {
        db_type: values.db_type ?? 'dm8',
        host: values.host,
        port: Number.parseInt(values.port, 10),
        username: values.username,
        password: values.password,
        schema: values.schema,
        export_schema: normalizeExportSchema(values.export_schema),
        database: values.database || undefined,
      };

      const result = await saveConnection(payload);
      if (!result.success || !result.data) {
        message.error(result.error || '保存配置失败');
        return;
      }

      const { config, source, updated_at } = result.data;
      const runtimeConfig: ConnectionConfig = { ...config, password: values.password };
      setConnectionConfig({ ...runtimeConfig, source, updated_at }, 'saved', updated_at ?? null, false);
      setLoadedFrom('saved', updated_at ?? null);
      setHasUnsavedChanges(false);
      setConnectionStatus(null);

      if (values.password) {
        message.success('配置已保存');
      } else {
        message.warning('配置已保存，请补充密码后再测试连接');
      }
    } catch {
      message.error('请先完成必填字段');
    } finally {
      setSaving(false);
    }
  };

  const handleNext = async () => {
    if (connectionStatus === 'success') {
      nextStep();
      return;
    }
    await handleTest();
  };

  useEffect(() => {
    if (initializedRef.current) {
      return;
    }
    initializedRef.current = true;

    const state = useExportStore.getState();
    if (state.connectionConfig) {
      const cfg = state.connectionConfig;
      form.setFieldsValue({ ...cfg, port: cfg.port?.toString() });
    } else {
      void loadSaved(false);
    }

    if (!state.driverInfo) {
      void getDriverInfo()
        .then((info) => {
          if (info) {
            setDriverInfo(info);
          }
        })
        .catch(() => undefined);
    }
  }, [form, loadSaved, setDriverInfo]);

  return (
    <TechCard delay={120}>
      <div className="connection-header">
        <SectionHeader
          title="数据库连接"
          subtitle={driverSummary}
        />
        <div className="connection-status">
          {connectionStatus === 'success' && (
            <span className="connection-ok">
              <CheckCircleOutlined />
              已连接
            </span>
          )}
          {connectionStatus === 'error' && (
            <span className="connection-fail">
              <CloseCircleOutlined />
              连接失败
            </span>
          )}
        </div>
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
          database: '',
        }}
        onValuesChange={(changedValues) => {
          if (changedValues.username !== undefined) {
            const currentSchema = form.getFieldValue('schema');
            if (!currentSchema) {
              form.setFieldValue('schema', changedValues.username);
            }
          }
          setHasUnsavedChanges(true);
          setLoadedFrom('manual', lastUpdatedAt);
          setConnectionStatus(null);
        }}
      >
        <Form.Item
          label="数据库类型"
          name="db_type"
          rules={[{ required: true, message: '请选择数据库类型' }]}
        >
          <Select
            options={dbTypeOptions}
            onChange={(value: DbType) => {
              const currentPort = form.getFieldValue('port');
              if (!currentPort || Object.values(dbPortDefaults).includes(currentPort)) {
                form.setFieldValue('port', dbPortDefaults[value]);
              }
            }}
          />
        </Form.Item>

        <Row gutter={16}>
          <Col xs={24} md={16}>
            <Form.Item label="主机地址" name="host" rules={[{ required: true, message: '请输入主机地址' }]}>
              <Input placeholder="localhost" />
            </Form.Item>
          </Col>
          <Col xs={24} md={8}>
            <Form.Item label="端口" name="port" rules={[{ required: true, message: '请输入端口' }]}>
              <Input placeholder="5236" />
            </Form.Item>
          </Col>
        </Row>

        {watchedDbType === 'shentong' && (
          <Form.Item label="实例名" name="database">
            <Input placeholder="OSRDB（默认实例）" />
          </Form.Item>
        )}

        <Row gutter={16}>
          <Col xs={24} md={12}>
            <Form.Item
              label="用户名"
              name="username"
              rules={[{ required: true, message: '请输入用户名' }]}
            >
              <Input placeholder="SYSDBA" />
            </Form.Item>
          </Col>
          <Col xs={24} md={12}>
            <Form.Item
              label="密码"
              name="password"
              rules={[{ required: true, message: '请输入密码' }]}
            >
              <Input.Password placeholder="******" />
            </Form.Item>
          </Col>
        </Row>

        <Form.Item label="源 Schema" name="schema" rules={[{ required: true, message: '请输入 Schema' }]}>
          <Input placeholder="默认与用户名一致" />
        </Form.Item>

        <Form.Item label="导出 Schema（可选）" name="export_schema">
          <Input placeholder="为空时使用源 Schema" />
        </Form.Item>

        <div className="connection-actions">
          <Space>
            <TechButton onClick={() => void loadSaved(true)} loading={loadingSaved} icon={<CloudDownloadOutlined />}>
              加载配置
            </TechButton>
            <TechButton onClick={handleSave} loading={saving} icon={<SaveOutlined />}>
              保存配置
            </TechButton>
          </Space>

          <Space>
            <TechButton onClick={handleTest} loading={loading} icon={<ThunderboltOutlined />} type="default">
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
          <p className="connection-unsaved">存在未保存更改</p>
        )}
      </Form>
    </TechCard>
  );
}
