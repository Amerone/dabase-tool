import React, { useEffect, useRef } from 'react';
import { Card, CardProps } from 'antd';
import { animate } from 'animejs';

// Helper for SVG path animation (replaces anime.setDashoffset)
const setDashoffset = (el: SVGGeometryElement | null) => {
  if (!el) return 0;
  const dashoffset = el.getTotalLength();
  el.setAttribute('stroke-dasharray', dashoffset.toString());
  return dashoffset;
}

interface TechCardProps extends CardProps {
  children: React.ReactNode;
  delay?: number;
}

export const TechCard: React.FC<TechCardProps> = ({ children, delay = 0, style, ...props }) => {
  const cardRef = useRef<HTMLDivElement>(null);
  const borderRef = useRef<SVGRectElement>(null);

  useEffect(() => {
    if (cardRef.current) {
      animate(cardRef.current, {
        opacity: [0, 1],
        translateY: [20, 0],
        duration: 800,
        delay: delay,
        easing: 'easeOutExpo'
      });
    }

    if (borderRef.current) {
      const offset = setDashoffset(borderRef.current);
      animate(borderRef.current, {
        strokeDashoffset: [offset, 0],
        easing: 'easeInOutSine',
        duration: 1500,
        delay: delay + 200,
        direction: 'alternate',
        loop: false
      });
    }
  }, [delay]);

  return (
    <div style={{ position: 'relative', padding: '2px' }}>
      {/* Decorative SVG Border */}
      <svg 
        style={{ 
          position: 'absolute', 
          top: 0, 
          left: 0, 
          width: '100%', 
          height: '100%', 
          pointerEvents: 'none', 
          zIndex: 0 
        }}
      >
        <rect 
          ref={borderRef}
          x="1" y="1" 
          width="99%" height="99%" 
          fill="none" 
          stroke="#00b96b" 
          strokeWidth="1" 
          strokeOpacity="0.3"
          rx="2"
        />
        {/* Corner Accents */}
        <path d="M 0 10 V 0 H 10" stroke="#00b96b" strokeWidth="2" fill="none" />
        <path d="M 100% 10 V 0 H calc(100% - 10px)" stroke="#00b96b" strokeWidth="2" fill="none" style={{ transform: 'translateX(100%)' }} /> {/* Note: SVG positioning needs exact coords, simplified here for responsive div */}
      </svg>
      
      {/* Corner Accents (Absolute Divs for easier positioning) */}
      <div style={{ position: 'absolute', top: -1, left: -1, width: 10, height: 10, borderTop: '2px solid #00b96b', borderLeft: '2px solid #00b96b', zIndex: 2 }} />
      <div style={{ position: 'absolute', top: -1, right: -1, width: 10, height: 10, borderTop: '2px solid #00b96b', borderRight: '2px solid #00b96b', zIndex: 2 }} />
      <div style={{ position: 'absolute', bottom: -1, left: -1, width: 10, height: 10, borderBottom: '2px solid #00b96b', borderLeft: '2px solid #00b96b', zIndex: 2 }} />
      <div style={{ position: 'absolute', bottom: -1, right: -1, width: 10, height: 10, borderBottom: '2px solid #00b96b', borderRight: '2px solid #00b96b', zIndex: 2 }} />

      <Card
        ref={cardRef as any}
        bordered={false}
        style={{
          background: 'rgba(5, 10, 15, 0.7)',
          backdropFilter: 'blur(10px)',
          opacity: 0, // Initial state for animation
          position: 'relative',
          zIndex: 1,
          ...style
        }}
        {...props}
      >
        {children}
      </Card>
    </div>
  );
};