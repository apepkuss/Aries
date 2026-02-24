import { useEffect, useRef, useState } from 'react';
import mermaid from 'mermaid';
import { cn } from '@/lib/utils';

// Initialize mermaid with default config
mermaid.initialize({
  startOnLoad: false,
  theme: 'default',
  securityLevel: 'loose',
  fontFamily: 'inherit',
});

interface MermaidDiagramProps {
  chart: string;
  className?: string;
}

export function MermaidDiagram({ chart, className }: MermaidDiagramProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [svg, setSvg] = useState<string>('');
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const renderChart = async () => {
      if (!chart.trim()) return;

      try {
        // Generate unique ID for mermaid
        const id = `mermaid-${Date.now()}-${Math.random().toString(36).substring(2, 9)}`;

        // Update theme based on dark mode
        const isDark = document.documentElement.classList.contains('dark');
        mermaid.initialize({
          startOnLoad: false,
          theme: isDark ? 'dark' : 'default',
          securityLevel: 'loose',
          fontFamily: 'inherit',
        });

        const { svg: renderedSvg } = await mermaid.render(id, chart.trim());
        setSvg(renderedSvg);
        setError(null);
      } catch (err) {
        console.error('Mermaid rendering error:', err);
        setError(err instanceof Error ? err.message : 'Failed to render diagram');
        setSvg('');
      }
    };

    renderChart();
  }, [chart]);

  if (error) {
    return (
      <div className={cn("p-4 rounded-lg bg-destructive/10 border border-destructive/20 text-sm text-destructive", className)}>
        <p className="font-medium mb-1">Diagram rendering error:</p>
        <pre className="text-xs overflow-x-auto">{error}</pre>
      </div>
    );
  }

  if (!svg) {
    return (
      <div className={cn("p-4 rounded-lg bg-muted animate-pulse", className)}>
        <div className="h-32 flex items-center justify-center text-muted-foreground text-sm">
          Loading diagram...
        </div>
      </div>
    );
  }

  return (
    <div
      ref={containerRef}
      className={cn(
        "my-4 p-4 rounded-xl bg-card border border-border/50 overflow-x-auto",
        "[&_svg]:max-w-full [&_svg]:h-auto",
        className
      )}
      dangerouslySetInnerHTML={{ __html: svg }}
    />
  );
}
