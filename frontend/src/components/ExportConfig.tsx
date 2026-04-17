import { useCallback, useEffect, useRef, useState } from 'react';
import {
  ClockCircleOutlined,
  CopyOutlined,
  DatabaseOutlined,
  FileTextOutlined,
  FolderOpenOutlined,
  RocketOutlined,
  SaveOutlined,
} from '@ant-design/icons';
import {
  Checkbox,
  Col,
  Form,
  Input,
  InputNumber,
  Progress,
  Row,
  Select,
  Space,
  Typography,
  message,
} from 'antd';
import { animate } from 'animejs';

import type { CapabilityLevel, ExportCapabilityReport, ExportRequest } from '@/types';
import { calcProgress } from '@/utils/exportProgress';
import {
  chooseExportDirectory,
  exportData,
  exportDDL,
  getExportCapabilities,
  getExportDirectory,
  saveExportDirectory,
} from '@/services/api';
import { useExportStore } from '@/store/useExportStore';
import { SectionHeader } from './common/SectionHeader';
import { TechButton } from './common/TechButton';
import { TechCard } from './common/TechCard';

const { Text } = Typography;

const capabilityRank: Record<CapabilityLevel, number> = {
  none: 0,
  partial: 1,
  full: 2,
};

const isSupportedLevel = (level?: CapabilityLevel) => {
  return (level ? capabilityRank[level] : 0) > capabilityRank.none;
};

type ExportOutcome = 'success' | 'partial' | 'failed' | null;

function isTauri() {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

function formatTime(durationMs: number) {
  const totalSeconds = Math.floor(durationMs / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes.toString().padStart(2, '0')}:${seconds.toString().padStart(2, '0')}`;
}

export default function ExportConfig() {
  const config = useExportStore((state) => state.connectionConfig);
  const selectedTables = useExportStore((state) => state.selectedTables);

  const [form] = Form.useForm();
  const watchedTargetDialect = Form.useWatch('target_dialect', form);
  const [loading, setLoading] = useState(false);
  const [progress, setProgress] = useState(0);
  const [animatedProgress, setAnimatedProgress] = useState(0);
  const [progressStatus, setProgressStatus] = useState<'normal' | 'active' | 'success' | 'exception'>(
    'normal'
  );
  const [elapsedTime, setElapsedTime] = useState(0);
  const [exportResult, setExportResult] = useState<{ ddl?: string; data?: string } | null>(null);
  const [exportOutcome, setExportOutcome] = useState<ExportOutcome>(null);
  const [includeRowCountsSupported, setIncludeRowCountsSupported] = useState(true);
  const [includeRowCountsNote, setIncludeRowCountsNote] = useState<string | null>(null);
  const [capabilityReport, setCapabilityReport] = useState<ExportCapabilityReport | null>(null);
  const [exportDirectory, setExportDirectory] = useState('');
  const [directoryLoading, setDirectoryLoading] = useState(false);
  const animatedProgressRef = useRef(0);
  const animatedProgressWholeRef = useRef(-1);
  const resultRef = useRef<HTMLDivElement>(null);

  const sourceDbType = config?.db_type ?? 'dm8';
  const targetDialect = watchedTargetDialect ?? sourceDbType;

  useEffect(() => {
    const animationObject = { value: animatedProgressRef.current };
    animate(animationObject, {
      value: progress,
      easing: 'easeOutExpo',
      duration: 600,
      round: 1,
      onUpdate: () => {
        const whole = Math.round(animationObject.value);
        animatedProgressRef.current = whole;
        if (whole !== animatedProgressWholeRef.current) {
          animatedProgressWholeRef.current = whole;
          setAnimatedProgress(whole);
        }
      },
    });
  }, [progress]);

  useEffect(() => {
    let timer: ReturnType<typeof setInterval> | undefined;
    if (loading) {
      const startTime = Date.now();
      timer = setInterval(() => {
        setElapsedTime(Date.now() - startTime);
      }, 500);
    }
    return () => clearInterval(timer);
  }, [loading]);

  useEffect(() => {
    if (config) {
      form.setFieldsValue({
        export_schema: config.export_schema ?? config.schema,
        target_dialect: config.db_type ?? 'dm8',
      });
    }
  }, [config, form]);

  useEffect(() => {
    if (exportResult && resultRef.current) {
      resultRef.current.scrollIntoView({ behavior: 'smooth', block: 'start' });
    }
  }, [exportResult]);

  useEffect(() => {
    let cancelled = false;
    setDirectoryLoading(true);
    void getExportDirectory()
      .then((response) => {
        if (!cancelled && response.success && response.data) {
          setExportDirectory(response.data);
        }
      })
      .finally(() => {
        if (!cancelled) {
          setDirectoryLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, []);

  const loadCapability = useCallback(
    async (source: typeof sourceDbType, target: typeof targetDialect) => {
      const response = await getExportCapabilities(source, target);
      if (!response.success || !response.data) {
        return { ok: false as const, error: response.error || '加载导出能力失败' };
      }
      setCapabilityReport(response.data);
      return { ok: true as const, data: response.data };
    },
    []
  );

  useEffect(() => {
    if (!config) {
      setCapabilityReport(null);
      setIncludeRowCountsSupported(false);
      setIncludeRowCountsNote(null);
      return;
    }

    let cancelled = false;
    void loadCapability(sourceDbType, targetDialect)
      .then((result) => {
        if (cancelled) {
          return;
        }
        if (!result.ok) {
          setIncludeRowCountsSupported(false);
          setIncludeRowCountsNote(result.error);
          form.setFieldValue('include_row_counts', false);
          return;
        }

        const supported = result.data.data_options?.include_row_counts_supported ?? false;
        const note = result.data.data_options?.include_row_counts_note ?? null;
        setIncludeRowCountsSupported(supported);
        setIncludeRowCountsNote(note);
        if (!supported) {
          form.setFieldValue('include_row_counts', false);
        }
      })
      .catch(() => {
        if (cancelled) {
          return;
        }
        setIncludeRowCountsSupported(false);
        setIncludeRowCountsNote('行数预扫描能力检查失败');
        form.setFieldValue('include_row_counts', false);
      });

    return () => {
      cancelled = true;
    };
  }, [config, form, loadCapability, sourceDbType, targetDialect]);

  const handleOpenFolder = async () => {
    const directory = exportDirectory.trim();
    if (!directory) {
      message.warning('请先设置导出目录');
      return;
    }
    if (isTauri()) {
      try {
        const { openPath } = await import('@tauri-apps/plugin-opener');
        await openPath(directory);
      } catch {
        message.error('无法打开导出目录');
      }
      return;
    }

    try {
      await navigator.clipboard.writeText(directory);
      message.success(`路径已复制: ${directory}`);
    } catch {
      message.info(`导出目录: ${directory}`);
    }
  };

  const persistExportDirectory = async (directory: string, showSuccess = true) => {
    const normalized = directory.trim();
    if (!normalized) {
      message.warning('导出目录不能为空');
      return null;
    }

    const response = await saveExportDirectory(normalized);
    if (!response.success || !response.data) {
      message.error(response.error || '保存导出目录失败');
      return null;
    }

    setExportDirectory(response.data);
    if (showSuccess) {
      message.success('导出目录已保存');
    }
    return response.data;
  };

  const handleChooseDirectory = async () => {
    setDirectoryLoading(true);
    try {
      const response = await chooseExportDirectory(exportDirectory.trim() || undefined);
      if (!response.success) {
        message.info(response.error || '浏览器模式请手动输入导出目录');
        return;
      }
      if (!response.data) {
        return;
      }
      await persistExportDirectory(response.data);
    } finally {
      setDirectoryLoading(false);
    }
  };

  const copyText = async (value: string, successMessage: string) => {
    try {
      await navigator.clipboard.writeText(value);
      message.success(successMessage);
    } catch {
      message.error('复制失败');
    }
  };

  const handleExport = async () => {
    if (!config || selectedTables.length === 0) {
      message.warning('请先完成连接并选择要导出的表');
      return;
    }

    try {
      const values = await form.validateFields();
      const savedExportDirectory = await persistExportDirectory(exportDirectory, false);
      if (!savedExportDirectory) {
        return;
      }

      const source = config.db_type ?? 'dm8';
      const target = values.target_dialect ?? source;

      let capability: ExportCapabilityReport | null = null;
      if (
        capabilityReport &&
        capabilityReport.source_db_type === source &&
        capabilityReport.target_dialect === target
      ) {
        capability = capabilityReport;
      } else {
        const capabilityResult = await loadCapability(source, target);
        capability = capabilityResult.ok ? capabilityResult.data : null;
      }

      if (!capability) {
        message.error('加载导出能力失败');
        return;
      }

      const ddlCapability = capability.entries.find((entry) => entry.object === 'ddl');
      const dataCapability = capability.entries.find((entry) => entry.object === 'data');

      if (values.include_ddl && !isSupportedLevel(ddlCapability?.effective_level)) {
        const reason = ddlCapability?.reason_code ? ` [${ddlCapability.reason_code}]` : '';
        message.warning(`${ddlCapability?.note || '当前组合不支持 DDL 导出'}${reason}`);
        return;
      }

      if (values.include_data && !isSupportedLevel(dataCapability?.effective_level)) {
        const reason = dataCapability?.reason_code ? ` [${dataCapability.reason_code}]` : '';
        message.warning(`${dataCapability?.note || '当前组合不支持数据导出'}${reason}`);
        return;
      }

      setLoading(true);
      setProgress(0);
      setProgressStatus('active');
      setElapsedTime(0);
      setExportResult(null);
      setExportOutcome(null);

      const request: ExportRequest = {
        config,
        target_dialect: target,
        export_schema: values.export_schema?.trim() || undefined,
        export_directory: savedExportDirectory,
        export_compat: values.export_compat,
        tables: selectedTables.map((name) => ({ schema: config.schema, name })),
        include_ddl: values.include_ddl,
        include_data: values.include_data,
        batch_size: values.batch_size || 1000,
        drop_existing: values.drop_existing,
        include_row_counts: includeRowCountsSupported ? Boolean(values.include_row_counts) : false,
        strict_mode: Boolean(values.strict_mode),
        identifier_case:
          target === 'kingbase' || target === 'shentong' ? values.identifier_case || 'lower' : undefined,
      };

      const results: { ddl?: string; data?: string } = {};
      let completedSteps = 0;
      let failedSteps = 0;
      const totalSteps = (values.include_ddl ? 1 : 0) + (values.include_data ? 1 : 0);

      const updateStepProgress = () => {
        const next = calcProgress({
          includeDdl: values.include_ddl,
          includeData: values.include_data,
          completedSteps,
          hasError: failedSteps > 0,
        });
        setProgress(next.percent);
        setProgressStatus(failedSteps > 0 ? 'exception' : next.status);
      };

      if (values.include_ddl) {
        const ddlResult = await exportDDL(request);
        if (ddlResult.success && ddlResult.data) {
          results.ddl = ddlResult.data.file_path;
          completedSteps += 1;
          message.success('DDL 导出成功');
        } else {
          failedSteps += 1;
          message.error(ddlResult.error || 'DDL 导出失败');
        }
        updateStepProgress();
      }

      if (values.include_data) {
        const dataResult = await exportData(request);
        if (dataResult.success && dataResult.data) {
          results.data = dataResult.data.file_path;
          completedSteps += 1;
          message.success('数据导出成功');
        } else {
          failedSteps += 1;
          message.error(dataResult.error || '数据导出失败');
        }
        updateStepProgress();
      }

      if (!values.include_ddl && !values.include_data) {
        setProgress(0);
        setProgressStatus('normal');
        message.warning('请至少选择一个导出模块');
      } else if (failedSteps === 0) {
        setExportOutcome('success');
        setExportResult(results.ddl || results.data ? results : null);
      } else if (failedSteps < totalSteps) {
        setExportOutcome('partial');
        setProgressStatus('exception');
        message.warning('导出部分成功，请根据报错信息重试失败步骤');
        setExportResult(results.ddl || results.data ? results : null);
      } else {
        setExportOutcome('failed');
        setProgressStatus('exception');
      }
    } catch {
      message.error('导出过程中发生异常');
      const errorProgress = calcProgress({
        includeDdl: true,
        includeData: true,
        completedSteps: 0,
        hasError: true,
      });
      setProgress(errorProgress.percent);
      setProgressStatus(errorProgress.status);
      setExportOutcome('failed');
    } finally {
      setLoading(false);
    }
  };

  return (
    <TechCard>
      <SectionHeader title="导出控制台" subtitle="设置导出策略并执行任务" />

      <div className="export-directory-panel">
        <div className="export-directory-header">
          <div>
            <p className="export-directory-title">导出目录</p>
            <p className="export-directory-note">导出前会保存该目录，下次自动使用。</p>
          </div>
        </div>
        <Space.Compact className="export-directory-control">
          <Input
            value={exportDirectory}
            onChange={(event) => setExportDirectory(event.target.value)}
            placeholder="请选择或输入服务器可访问的绝对路径"
            disabled={directoryLoading || loading}
          />
          <TechButton
            icon={<FolderOpenOutlined />}
            onClick={handleChooseDirectory}
            loading={directoryLoading}
            disabled={loading}
          >
            选择目录
          </TechButton>
          <TechButton
            icon={<SaveOutlined />}
            onClick={() => void persistExportDirectory(exportDirectory)}
            disabled={directoryLoading || loading}
          >
            保存
          </TechButton>
        </Space.Compact>
      </div>

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
        <Row gutter={16}>
          <Col xs={24} md={12}>
            <Form.Item name="include_ddl" valuePropName="checked">
              <Checkbox>
                <FileTextOutlined /> 导出 DDL（表结构）
              </Checkbox>
            </Form.Item>
          </Col>
          <Col xs={24} md={12}>
            <Form.Item name="include_data" valuePropName="checked">
              <Checkbox>
                <DatabaseOutlined /> 导出数据（INSERT）
              </Checkbox>
            </Form.Item>
          </Col>
        </Row>

        <Row gutter={16}>
          <Col xs={24} md={12}>
            <Form.Item name="drop_existing" valuePropName="checked">
              <Checkbox>生成 DROP TABLE IF EXISTS</Checkbox>
            </Form.Item>
          </Col>
          <Col xs={24} md={12}>
            <Form.Item name="include_row_counts" valuePropName="checked">
              <Checkbox disabled={!includeRowCountsSupported}>写入行数预扫描注释</Checkbox>
            </Form.Item>
            {!includeRowCountsSupported && includeRowCountsNote && (
              <Text type="secondary" style={{ fontSize: 12 }}>
                {includeRowCountsNote}
              </Text>
            )}
          </Col>
        </Row>

        <Form.Item name="strict_mode" valuePropName="checked">
          <Checkbox>严格模式（部分支持时快速失败）</Checkbox>
        </Form.Item>

        <Form.Item label="批次大小（每次插入行数）" name="batch_size">
          <InputNumber min={100} max={10000} step={100} style={{ width: '100%' }} />
        </Form.Item>

        <Form.Item
          label="目标方言"
          name="target_dialect"
          rules={[{ required: true, message: '请选择目标方言' }]}
        >
          <Select
            options={[
              { value: 'dm8', label: 'DM8' },
              { value: 'mysql', label: 'MySQL' },
              { value: 'kingbase', label: 'KingbaseES' },
              { value: 'shentong', label: 'ShenTong/OSCAR' },
            ]}
          />
        </Form.Item>

        <Form.Item
          label="导出兼容模式"
          name="export_compat"
          rules={[{ required: true, message: '请选择兼容模式' }]}
        >
          <Select
            placeholder="选择兼容模式"
            options={[
              { value: 'datagrip', label: 'DataGrip 逐语句模式（END; 无 /）' },
              { value: 'datagrip-script', label: 'DataGrip 脚本模式（触发器单独输出）' },
              { value: 'script', label: 'DBeaver/SQLark/DIsql 模式（END; + /）' },
            ]}
          />
        </Form.Item>

        {(targetDialect === 'kingbase' || targetDialect === 'shentong') && (
          <Form.Item label="标识符大小写" name="identifier_case" initialValue="lower">
            <Select
              options={[
                { value: 'lower', label: targetDialect === 'kingbase' ? '小写（Kingbase 推荐）' : '小写' },
                { value: 'upper', label: '大写' },
                { value: 'preserve', label: '保持原样' },
              ]}
            />
          </Form.Item>
        )}

        <Form.Item label="导出 Schema（可选）" name="export_schema">
          <Input placeholder="为空时使用源 Schema" />
        </Form.Item>

        <Space direction="vertical" style={{ width: '100%', marginTop: 20 }}>
          <TechButton
            type="primary"
            icon={<RocketOutlined />}
            onClick={handleExport}
            loading={loading}
            disabled={!config || selectedTables.length === 0}
            block
            style={{ height: 48, fontWeight: 700 }}
          >
            {loading ? '导出中...' : '开始导出'}
          </TechButton>

          {(loading || progress > 0) && (
            <div className="export-progress-panel">
              <div className="export-progress-header">
                <Text type="secondary">进度</Text>
                <Text type="secondary">
                  <ClockCircleOutlined /> {formatTime(elapsedTime)}
                </Text>
              </div>
              <Progress
                percent={animatedProgress}
                status={loading ? 'active' : progressStatus}
                strokeColor={{ '0%': '#4aa8ff', '100%': '#14c8be' }}
                trailColor="rgba(255,255,255,0.1)"
              />
            </div>
          )}

          {exportResult && (
            <div
              ref={resultRef}
              className={`export-result-panel ${
                exportOutcome === 'partial' ? 'export-result-partial' : 'export-result-success'
              }`}
            >
              <h4>{exportOutcome === 'partial' ? '导出部分成功' : '导出完成'}</h4>
              <Space direction="vertical" style={{ width: '100%' }}>
                {exportOutcome === 'partial' && <p>存在失败步骤，请按错误提示重试。</p>}
                {exportResult.ddl && (
                  <div className="export-result-item">
                    <span>DDL 文件:</span>
                    <code>{exportResult.ddl}</code>
                    <CopyOutlined onClick={() => void copyText(exportResult.ddl!, 'DDL 路径已复制')} />
                  </div>
                )}
                {exportResult.data && (
                  <div className="export-result-item">
                    <span>数据文件:</span>
                    <code>{exportResult.data}</code>
                    <CopyOutlined onClick={() => void copyText(exportResult.data!, '数据路径已复制')} />
                  </div>
                )}
                <p className="export-result-duration">总耗时: {formatTime(elapsedTime)}</p>
                <TechButton icon={<FolderOpenOutlined />} onClick={handleOpenFolder}>
                  打开导出目录
                </TechButton>
              </Space>
            </div>
          )}
        </Space>
      </Form>
    </TechCard>
  );
}
