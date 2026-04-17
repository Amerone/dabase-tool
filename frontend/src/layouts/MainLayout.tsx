import { useEffect, useRef } from 'react';
import { Layout, Typography } from 'antd';
import { Outlet } from 'react-router-dom';
import { animate, stagger } from 'animejs';

import TechBackground from '@/components/TechBackground';

const { Header, Content, Footer } = Layout;
const { Title } = Typography;

export default function MainLayout() {
  const shellRef = useRef<HTMLDivElement>(null);
  const brandRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (shellRef.current) {
      animate(shellRef.current, {
        opacity: [0, 1],
        translateY: [16, 0],
        duration: 700,
        easing: 'easeOutExpo',
      });
    }

    if (brandRef.current?.children.length) {
      animate(brandRef.current.children, {
        opacity: [0, 1],
        translateY: [10, 0],
        delay: stagger(90),
        duration: 520,
        easing: 'easeOutQuad',
      });
    }
  }, []);

  return (
    <>
      <TechBackground />
      <div ref={shellRef} className="app-shell">
        <Layout className="app-layout">
          <Header className="app-header">
            <div ref={brandRef} className="app-brand">
              <Title level={3} className="app-title">
                Amarone Data Bridge
              </Title>
            </div>
          </Header>

          <Content className="app-content">
            <div className="app-content-inner">
              <Outlet />
            </div>
          </Content>

          <Footer className="app-footer">
            Amarone {new Date().getFullYear()} · Data migration tooling
          </Footer>
        </Layout>
      </div>
    </>
  );
}
