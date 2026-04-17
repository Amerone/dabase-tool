import { Button } from 'antd';
import type { ButtonProps } from 'antd';

interface TechButtonProps extends ButtonProps {
  glow?: boolean;
}

export function TechButton({ className, glow = true, ...props }: TechButtonProps) {
  return (
    <Button
      {...props}
      className={['tech-button', glow ? 'tech-button-glow' : '', className].filter(Boolean).join(' ')}
    />
  );
}
