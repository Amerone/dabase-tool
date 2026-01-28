import React, { useRef } from 'react';
import { Button, ButtonProps } from 'antd';
import { animate } from 'animejs';

interface TechButtonProps extends ButtonProps {
  glitch?: boolean;
}

export const TechButton: React.FC<TechButtonProps> = ({ children, glitch = true, style, ...props }) => {
  const btnRef = useRef<HTMLButtonElement>(null);

  const handleMouseEnter = () => {
    if (!btnRef.current || !glitch) return;
    
    animate(btnRef.current, {
      scale: 1.05,
      duration: 400,
      easing: 'easeOutElastic(1, .8)'
    });
  };

  const handleMouseLeave = () => {
    if (!btnRef.current) return;
    animate(btnRef.current, {
      scale: 1,
      duration: 300,
      easing: 'easeOutQuad'
    });
  };

  const handleClick = (e: React.MouseEvent<HTMLElement, MouseEvent>) => {
    if (props.onClick) props.onClick(e);
    if (!btnRef.current) return;
    
    // Click ripple/shockwave effect could go here
    animate(btnRef.current, {
      scale: [0.95, 1],
      duration: 100,
      easing: 'easeInOutQuad'
    });
  };

  return (
    <Button
      ref={btnRef as any}
      {...props}
      onMouseEnter={handleMouseEnter}
      onMouseLeave={handleMouseLeave}
      onClick={handleClick}
      style={{
        position: 'relative',
        overflow: 'hidden',
        border: '1px solid rgba(0, 185, 107, 0.5)',
        textTransform: 'uppercase',
        letterSpacing: '1px',
        fontWeight: 600,
        ...style
      }}
    >
      {children}
      {/* Glitch overlay element could be added here */}
    </Button>
  );
};