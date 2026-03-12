import React, { useRef } from 'react'
import { Button } from 'antd'
import type { ButtonProps } from 'antd'
import { animate } from 'animejs'

interface TechButtonProps extends ButtonProps {
  glitch?: boolean
}

export const TechButton: React.FC<TechButtonProps> = ({
  children,
  glitch = true,
  style,
  ...props
}) => {
  const btnRef = useRef<HTMLButtonElement | HTMLAnchorElement>(null)

  const handleMouseEnter = () => {
    if (!btnRef.current || !glitch) return

    animate(btnRef.current, {
      scale: 1.05,
      duration: 400,
      easing: 'easeOutElastic(1, .8)',
    })
  }

  const handleMouseLeave = () => {
    if (!btnRef.current) return
    animate(btnRef.current, {
      scale: 1,
      duration: 300,
      easing: 'easeOutQuad',
    })
  }

  const handleClick = (e: React.MouseEvent<HTMLElement, MouseEvent>) => {
    props.onClick?.(e)
    if (!btnRef.current) return

    animate(btnRef.current, {
      scale: [0.95, 1],
      duration: 100,
      easing: 'easeInOutQuad',
    })
  }

  return (
    <Button
      ref={btnRef as React.Ref<HTMLButtonElement | HTMLAnchorElement>}
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
        ...style,
      }}
    >
      {children}
    </Button>
  )
}
