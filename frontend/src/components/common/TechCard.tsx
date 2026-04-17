import { useEffect, useRef } from 'react';
import { Card } from 'antd';
import type { CardProps } from 'antd';
import { animate } from 'animejs';

interface TechCardProps extends CardProps {
  children: React.ReactNode;
  delay?: number;
}

export function TechCard({ children, delay = 0, className, ...props }: TechCardProps) {
  const shellRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!shellRef.current) {
      return;
    }

    animate(shellRef.current, {
      opacity: [0, 1],
      translateY: [12, 0],
      duration: 520,
      delay,
      easing: 'easeOutQuad',
    });
  }, [delay]);

  return (
    <div ref={shellRef} className="tech-card-shell">
      <Card className={['tech-card', className].filter(Boolean).join(' ')} bordered={false} {...props}>
        {children}
      </Card>
    </div>
  );
}
