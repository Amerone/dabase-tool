import { Suspense, lazy, useMemo } from 'react';
import { Col, Row, Space, Steps } from 'antd';
import {
  DatabaseOutlined,
  ExportOutlined,
  LeftOutlined,
  RightOutlined,
  TableOutlined,
} from '@ant-design/icons';

import { useExportStore } from '@/store/useExportStore';
import { TechButton } from '@/components/common/TechButton';

const ConnectionForm = lazy(() => import('@/components/ConnectionForm'));
const SchemaExplorer = lazy(() => import('@/components/SchemaExplorer'));
const TableSelector = lazy(() => import('@/components/TableSelector'));
const ExportConfig = lazy(() => import('@/components/ExportConfig'));

export default function ExportWizard() {
  const currentStep = useExportStore((state) => state.currentStep);
  const nextStep = useExportStore((state) => state.nextStep);
  const prevStep = useExportStore((state) => state.prevStep);
  const isConnected = useExportStore((state) => state.isConnected);
  const selectedTables = useExportStore((state) => state.selectedTables);
  const tables = useExportStore((state) => state.tables);

  const rowCountMap = useMemo(() => {
    return new Map(tables.map((table) => [table.name, table.row_count ?? 0]));
  }, [tables]);

  const totalRows = useMemo(() => {
    return selectedTables.reduce((acc, tableName) => acc + (rowCountMap.get(tableName) ?? 0), 0);
  }, [rowCountMap, selectedTables]);

  const steps = useMemo(
    () => [
      {
        title: '连接',
        icon: <DatabaseOutlined />,
        content: <ConnectionForm />,
      },
      {
        title: '选表',
        icon: <TableOutlined />,
        content: (
          <Row gutter={[24, 24]}>
            <Col xs={24} lg={16}>
              <SchemaExplorer />
            </Col>
            <Col xs={24} lg={8}>
              <TableSelector />
            </Col>
          </Row>
        ),
      },
      {
        title: '导出',
        icon: <ExportOutlined />,
        content: <ExportConfig />,
      },
    ],
    []
  );

  const loadingFallback = <div className="wizard-loading">模块加载中...</div>;

  return (
    <div className="wizard-shell">
      <div className="wizard-steps-panel">
        <Steps
          current={currentStep}
          items={steps.map((step) => ({ title: step.title, icon: step.icon }))}
          className="wizard-steps"
        />
      </div>

      <div className="wizard-stage">
        <Suspense fallback={loadingFallback}>{steps[currentStep].content}</Suspense>
      </div>

      <div className="wizard-footer">
        <div className="wizard-footer-state">
          {currentStep === 0 && <span>状态: 请完成连接测试</span>}
          {currentStep === 1 && (
            <Space size="large">
              <span>
                已选表数 <strong>{selectedTables.length}</strong>
              </span>
              <span>
                预估总行数 <strong>{totalRows.toLocaleString()}</strong>
              </span>
            </Space>
          )}
          {currentStep === 2 && <span>状态: 已可执行导出</span>}
        </div>

        <Space>
          {currentStep > 0 && (
            <TechButton onClick={prevStep} size="large" icon={<LeftOutlined />}>
              上一步
            </TechButton>
          )}
          {currentStep < steps.length - 1 && (
            <TechButton
              type="primary"
              onClick={nextStep}
              size="large"
              disabled={(currentStep === 0 && !isConnected) || (currentStep === 1 && selectedTables.length === 0)}
              style={{ minWidth: 140 }}
            >
              下一步
              <RightOutlined />
            </TechButton>
          )}
        </Space>
      </div>
    </div>
  );
}
