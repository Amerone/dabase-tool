import { useEffect, useRef } from 'react';
import { animate } from 'animejs';

export default function TechBackground() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const mouseRef = useRef({ x: -1000, y: -1000 });

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    let width = canvas.width = window.innerWidth;
    let height = canvas.height = window.innerHeight;

    const resize = () => {
      width = canvas.width = window.innerWidth;
      height = canvas.height = window.innerHeight;
    };
    window.addEventListener('resize', resize);

    const handleMouseMove = (e: MouseEvent) => {
      mouseRef.current = { x: e.clientX, y: e.clientY };
    };
    window.addEventListener('mousemove', handleMouseMove);

    // Tech particles
    const particles: any[] = [];
    const particleCount = Math.min(width / 10, 150); // Increased density

    for (let i = 0; i < particleCount; i++) {
      particles.push({
        x: Math.random() * width,
        y: Math.random() * height,
        radius: Math.random() < 0.1 ? Math.random() * 2 + 1 : Math.random() * 1.5, // Some larger nodes
        vx: (Math.random() - 0.5) * 0.3,
        vy: (Math.random() - 0.5) * 0.3,
        alpha: Math.random() * 0.5 + 0.1,
        baseAlpha: Math.random() * 0.5 + 0.1,
        pulseSpeed: Math.random() * 0.05 + 0.01
      });
    }

    let time = 0;

    const animateParticles = () => {
      if (!ctx) return;
      time++;
      ctx.clearRect(0, 0, width, height);
      
      // Draw Hexagonal Grid (Subtle background)
      ctx.strokeStyle = 'rgba(0, 185, 107, 0.03)';
      ctx.lineWidth = 1;
      const gridSize = 60;
      
      // Moving grid effect
      const offsetX = (time * 0.2) % gridSize;

      // Vertical lines
      for (let x = offsetX; x < width; x += gridSize) {
        ctx.beginPath();
        ctx.moveTo(x, 0);
        ctx.lineTo(x, height);
        ctx.stroke();
      }
      
      // Horizontal scanline effect
      const scanY = (time * 2) % height;
      const scanHeight = 50;
      const gradient = ctx.createLinearGradient(0, scanY, 0, scanY + scanHeight);
      gradient.addColorStop(0, 'rgba(0, 185, 107, 0)');
      gradient.addColorStop(0.5, 'rgba(0, 185, 107, 0.05)');
      gradient.addColorStop(1, 'rgba(0, 185, 107, 0)');
      ctx.fillStyle = gradient;
      ctx.fillRect(0, scanY, width, scanHeight);


      // Update and Draw Particles
      particles.forEach((p, i) => {
        p.x += p.vx;
        p.y += p.vy;

        // Pulse alpha
        p.alpha = p.baseAlpha + Math.sin(time * p.pulseSpeed) * 0.2;

        // Mouse interaction (Repulsion/Attraction)
        const dxMouse = p.x - mouseRef.current.x;
        const dyMouse = p.y - mouseRef.current.y;
        const distMouse = Math.sqrt(dxMouse * dxMouse + dyMouse * dyMouse);
        
        if (distMouse < 200) {
            // Gentle attraction
            p.vx -= dxMouse * 0.0001;
            p.vy -= dyMouse * 0.0001;
            p.alpha = Math.min(p.alpha + 0.3, 1); // Brighten near mouse
        }

        // Boundary wrap
        if (p.x < 0) p.x = width;
        if (p.x > width) p.x = 0;
        if (p.y < 0) p.y = height;
        if (p.y > height) p.y = 0;

        // Draw Particle
        ctx.beginPath();
        ctx.arc(p.x, p.y, p.radius, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(0, 185, 107, ${p.alpha})`; // Green tint
        ctx.fill();

        // Connect nearby particles
        for (let j = i + 1; j < particles.length; j++) {
          const p2 = particles[j];
          const dx = p.x - p2.x;
          const dy = p.y - p2.y;
          const dist = Math.sqrt(dx * dx + dy * dy);

          if (dist < 120) {
            ctx.beginPath();
            // Opacity based on distance
            ctx.strokeStyle = `rgba(0, 185, 107, ${0.15 * (1 - dist / 120)})`;
            ctx.lineWidth = 0.5;
            ctx.moveTo(p.x, p.y);
            ctx.lineTo(p2.x, p2.y);
            ctx.stroke();
          }
        }
        
        // Connect to mouse
        if (distMouse < 150) {
            ctx.beginPath();
            ctx.strokeStyle = `rgba(0, 185, 107, ${0.2 * (1 - distMouse / 150)})`;
            ctx.lineWidth = 0.8;
            ctx.moveTo(p.x, p.y);
            ctx.lineTo(mouseRef.current.x, mouseRef.current.y);
            ctx.stroke();
        }
      });

      requestAnimationFrame(animateParticles);
    };

    const anim = requestAnimationFrame(animateParticles);

    // Anime.js entrance
    animate(canvas, {
      opacity: [0, 1],
      duration: 1500,
      easing: 'easeOutExpo'
    });

    return () => {
      window.removeEventListener('resize', resize);
      window.removeEventListener('mousemove', handleMouseMove);
      cancelAnimationFrame(anim);
    };
  }, []);

  return (
    <canvas
      ref={canvasRef}
      style={{
        position: 'fixed',
        top: 0,
        left: 0,
        zIndex: 0,
        pointerEvents: 'none',
        background: 'radial-gradient(circle at 50% 50%, #0a1929 0%, #000000 100%)' // Deeper blue-black
      }}
    />
  );
}