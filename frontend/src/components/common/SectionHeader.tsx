import React from 'react';
import { Typography } from 'antd';

const { Title } = Typography;

interface SectionHeaderProps {
  title: string;
  subtitle?: string;
}

export const SectionHeader: React.FC<SectionHeaderProps> = ({ title, subtitle }) => {
  return (
    <div style={{ marginBottom: 24, borderLeft: '4px solid #00b96b', paddingLeft: 16 }}>
      <Title level={4} style={{ margin: 0, color: '#fff', fontFamily: 'Orbitron, sans-serif' }}>
        <span style={{ color: '#00b96b', marginRight: 8 }}>&gt;_</span>
        {title}
      </Title>
      {subtitle && (
        <div style={{ color: 'rgba(255,255,255,0.5)', marginTop: 4, fontFamily: 'JetBrains Mono, monospace', fontSize: '12px' }}>
          // {subtitle}
        </div>
      )}
    </div>
  );
};
