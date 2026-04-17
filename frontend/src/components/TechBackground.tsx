import { useEffect, useRef } from 'react';

type Particle = {
  x: number;
  y: number;
  radius: number;
  vx: number;
  vy: number;
  alpha: number;
  phase: number;
};

function createParticles(width: number, height: number, count: number): Particle[] {
  return Array.from({ length: count }, () => ({
    x: Math.random() * width,
    y: Math.random() * height,
    radius: Math.random() * 1.6 + 0.6,
    vx: (Math.random() - 0.5) * 0.2,
    vy: (Math.random() - 0.5) * 0.2,
    alpha: Math.random() * 0.4 + 0.2,
    phase: Math.random() * Math.PI * 2,
  }));
}

function createBackdropGradient(
  context: CanvasRenderingContext2D,
  width: number,
  height: number
): CanvasGradient {
  const gradient = context.createRadialGradient(
    width * 0.7,
    height * 0.12,
    0,
    width * 0.5,
    height * 0.5,
    Math.max(width, height)
  );
  gradient.addColorStop(0, 'rgba(34, 77, 95, 0.42)');
  gradient.addColorStop(0.45, 'rgba(11, 21, 34, 0.82)');
  gradient.addColorStop(1, 'rgba(4, 7, 14, 1)');
  return gradient;
}

export default function TechBackground() {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) {
      return;
    }

    const context = canvas.getContext('2d');
    if (!context) {
      return;
    }

    const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    let width = window.innerWidth;
    let height = window.innerHeight;
    let animationFrameId = 0;
    let lastFrame = 0;
    const targetFrameInterval = reduceMotion ? 64 : 33;
    let running = false;
    let backdropGradient = createBackdropGradient(context, width, height);

    const resolveParticleCount = () => {
      const base = Math.round((width * height) / 42000);
      const capped = Math.max(18, Math.min(base, 70));
      return reduceMotion ? Math.max(10, Math.floor(capped / 2)) : capped;
    };

    const resize = () => {
      width = window.innerWidth;
      height = window.innerHeight;
      canvas.width = width;
      canvas.height = height;
      backdropGradient = createBackdropGradient(context, width, height);
      particles = createParticles(width, height, resolveParticleCount());
    };

    canvas.width = width;
    canvas.height = height;
    let particles = createParticles(width, height, resolveParticleCount());

    const drawBackdrop = () => {
      context.fillStyle = backdropGradient;
      context.fillRect(0, 0, width, height);
    };

    const render = (timestamp: number) => {
      if (!running) {
        return;
      }
      if (timestamp - lastFrame < targetFrameInterval) {
        animationFrameId = requestAnimationFrame(render);
        return;
      }
      lastFrame = timestamp;

      drawBackdrop();

      for (let i = 0; i < particles.length; i += 1) {
        const p = particles[i];
        p.x += p.vx;
        p.y += p.vy;
        p.phase += 0.02;

        if (p.x < -8) p.x = width + 8;
        if (p.x > width + 8) p.x = -8;
        if (p.y < -8) p.y = height + 8;
        if (p.y > height + 8) p.y = -8;

        const alpha = p.alpha + Math.sin(p.phase) * 0.12;
        context.beginPath();
        context.arc(p.x, p.y, p.radius, 0, Math.PI * 2);
        context.fillStyle = `rgba(118, 255, 229, ${Math.max(0.08, alpha)})`;
        context.fill();

        // Bound line checks to nearby indices to avoid O(n^2) work.
        const maxNeighbor = Math.min(i + 8, particles.length - 1);
        for (let j = i + 1; j <= maxNeighbor; j += 1) {
          const p2 = particles[j];
          const dx = p.x - p2.x;
          const dy = p.y - p2.y;
          const distance = Math.hypot(dx, dy);
          if (distance > 120) {
            continue;
          }
          context.beginPath();
          context.moveTo(p.x, p.y);
          context.lineTo(p2.x, p2.y);
          context.strokeStyle = `rgba(91, 201, 255, ${0.12 * (1 - distance / 120)})`;
          context.lineWidth = 0.6;
          context.stroke();
        }
      }

      animationFrameId = requestAnimationFrame(render);
    };

    const start = () => {
      if (running) {
        return;
      }
      running = true;
      lastFrame = performance.now();
      animationFrameId = requestAnimationFrame(render);
    };

    const stop = () => {
      if (!running) {
        return;
      }
      running = false;
      cancelAnimationFrame(animationFrameId);
    };

    const handleVisibilityChange = () => {
      if (document.hidden) {
        stop();
      } else {
        start();
      }
    };

    window.addEventListener('resize', resize);
    document.addEventListener('visibilitychange', handleVisibilityChange);
    if (!document.hidden) {
      start();
    }

    return () => {
      stop();
      window.removeEventListener('resize', resize);
      document.removeEventListener('visibilitychange', handleVisibilityChange);
    };
  }, []);

  return <canvas ref={canvasRef} className="app-background-canvas" />;
}
