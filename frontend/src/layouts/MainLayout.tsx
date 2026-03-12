import { useEffect, useRef } from 'react';
import { Layout, Typography, theme } from 'antd';
import { Outlet } from 'react-router-dom';
import { animate, stagger, splitText } from 'animejs';
import TechBackground from '@/components/TechBackground';

const { Header, Content, Footer } = Layout;
const { Title } = Typography;

export default function MainLayout() {
  const {
    token: { borderRadiusLG },
  } = theme.useToken();
  
  const layoutRef = useRef<HTMLDivElement>(null);
  const titleRef = useRef<HTMLHeadingElement>(null);

  useEffect(() => {
    if (!layoutRef.current) return;

    // Animate layout entrance
    animate(layoutRef.current, {
      opacity: [0, 1],
      translateY: [20, 0],
      duration: 800,
      easing: 'easeOutExpo',
      delay: 200
    });

    if (titleRef.current) {
      const { chars } = splitText(titleRef.current, { words: false, chars: true });
      
      animate(chars, {
        // Property keyframes
        y: [
          { to: '-1.2rem', ease: 'outExpo', duration: 600 },
          { to: 0, ease: 'outBounce', duration: 800, delay: 100 }
        ],
        // Property specific parameters
        rotate: {
          from: '-1turn',
          delay: 0
        },
        delay: stagger(50),
        ease: 'inOutCirc',
        loopDelay: 1000,
        loop: true
      });
    }
  }, []);

  return (
    <>
      <TechBackground />
      <div ref={layoutRef} style={{ position: 'relative', zIndex: 1, opacity: 0 }}>
        <Layout style={{ height: '100vh', background: 'transparent', display: 'flex', flexDirection: 'column' }}>
          <Header style={{ display: 'flex', alignItems: 'center', background: 'rgba(0, 0, 0, 0.5)', backdropFilter: 'blur(10px)' }}>
            <Title
              ref={titleRef}
              level={3}
              style={{ color: '#00b96b', margin: 0, textShadow: '0 0 10px rgba(0, 185, 107, 0.5)', display: 'inline-block' }}
            >
              Amarone
            </Title>
          </Header>
          <Content style={{ padding: '0 24px', marginTop: 16, flex: 1, overflow: 'auto' }}>
            <div
              style={{
                minHeight: 280,
                padding: '16px 0',
                borderRadius: borderRadiusLG,
              }}
            >
              <Outlet />
            </div>
          </Content>
          <Footer style={{ textAlign: 'center', color: 'rgba(255,255,255,0.5)', padding: '8px 24px', fontSize: 12 }}>
            Amarone &copy;{new Date().getFullYear()} // 系统就绪
          </Footer>
        </Layout>
      </div>
    </>
  );
}
