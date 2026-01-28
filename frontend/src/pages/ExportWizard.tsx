import { useEffect, useRef, useLayoutEffect } from 'react';
import { Steps, Space, Row, Col } from 'antd';
import { DatabaseOutlined, TableOutlined, ExportOutlined, LeftOutlined, RightOutlined } from '@ant-design/icons';
import { createLayout } from 'animejs';
import { useExportStore } from '@/store/useExportStore';
import ConnectionForm from '@/components/ConnectionForm';
import SchemaExplorer from '@/components/SchemaExplorer';
import TableSelector from '@/components/TableSelector';
import ExportConfig from '@/components/ExportConfig';
import { TechButton } from '@/components/common/TechButton';

export default function ExportWizard() {
  const currentStep = useExportStore((state) => state.currentStep);
  const nextStep = useExportStore((state) => state.nextStep);
  const prevStep = useExportStore((state) => state.prevStep);
  const isConnected = useExportStore((state) => state.isConnected);
  const selectedTables = useExportStore((state) => state.selectedTables);
  const tables = useExportStore((state) => state.tables);
  
  const contentRef = useRef<HTMLDivElement>(null);
  const layoutRef = useRef<any>(null);

  const totalRows = selectedTables.reduce((acc, tableName) => {
    const table = tables.find(t => t.name === tableName);
    return acc + (table?.row_count ?? 0);
  }, 0);

  useEffect(() => {
    if (contentRef.current && !layoutRef.current) {
      layoutRef.current = createLayout(contentRef.current);
    }
  }, []);

  useLayoutEffect(() => {
    if (layoutRef.current) {
      layoutRef.current.animate();
    }
  }, [currentStep]);

  const steps = [
    {
      title: '连接',
      icon: <DatabaseOutlined />,
      content: <ConnectionForm />,
    },
    {
      title: '选择',
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
  ];

  const handleNext = () => {
    layoutRef.current?.record();
    nextStep();
  };

  const handlePrev = () => {
    layoutRef.current?.record();
    prevStep();
  };

  return (
    <div>
      <div style={{ marginBottom: 32, padding: '0 24px' }}>
        <Steps 
          current={currentStep} 
          items={steps.map(s => ({ title: s.title, icon: s.icon }))} 
          className="tech-steps"
        />
        <style>{`
          .tech-steps .ant-steps-item-title {
            font-family: 'Orbitron', sans-serif !important;
            letter-spacing: 1px;
          }
          .tech-steps .ant-steps-item-process .ant-steps-item-icon {
            background: #00b96b;
            border-color: #00b96b;
          }
        `}</style>
      </div>
      
      <div ref={contentRef} style={{ minHeight: '400px', marginBottom: '100px' }}>
        {steps[currentStep].content}
      </div>

      <div style={{ 
        padding: '16px 48px',
        background: 'rgba(5, 10, 15, 0.85)', 
        backdropFilter: 'blur(20px)',
        borderTop: '1px solid rgba(0, 185, 107, 0.3)',
        boxShadow: '0 -10px 30px rgba(0,0,0,0.5)',
        position: 'fixed',
        bottom: 0,
        left: 0,
        right: 0,
        zIndex: 100,
        display: 'flex',
        justifyContent: 'space-between',
        alignItems: 'center'
      }}>
        <div style={{ fontFamily: 'JetBrains Mono', fontSize: '12px', color: 'rgba(255,255,255,0.6)' }}>
          {currentStep === 1 && (
            <Space size="large">
              <span>已选对象: <span style={{ color: '#00b96b', fontWeight: 'bold', fontSize: '14px' }}>{selectedTables.length}</span></span>
              <span>数据量级: <span style={{ color: '#00b96b', fontWeight: 'bold', fontSize: '14px' }}>{totalRows.toLocaleString()}</span></span>
            </Space>
          )}
          {currentStep === 0 && <span>状态: 等待链路接入...</span>}
          {currentStep === 2 && <span>状态: 就绪可导出</span>}
        </div>
        <Space>
          {currentStep > 0 && (
            <TechButton onClick={handlePrev} size="large" icon={<LeftOutlined />} type="default">
              返回
            </TechButton>
          )}
          {currentStep < steps.length - 1 && (
            <TechButton 
              type="primary" 
              onClick={handleNext}
              size="large"
              disabled={
                (currentStep === 0 && !isConnected) ||
                (currentStep === 1 && selectedTables.length === 0)
              }
              style={{ minWidth: 140 }}
            >
              下一步 <RightOutlined />
            </TechButton>
          )}
        </Space>
      </div>
    </div>
  );
}