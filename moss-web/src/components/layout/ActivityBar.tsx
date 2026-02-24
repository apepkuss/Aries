import { MessageSquare, Blocks, Server } from 'lucide-react';
import { Tooltip, TooltipTrigger, TooltipContent } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import { useUIStore, useConfigStore } from '@/stores';

type ActiveView = 'chat' | 'skills' | 'mcp';

interface ActivityItem {
  id: ActiveView;
  icon: React.ComponentType<{ className?: string }>;
  label: string;
}

export function ActivityBar() {
  const { activeView, sidebarOpen, handleActivityBarClick } = useUIStore();
  const { config } = useConfigStore();

  const skillsEnabled = config?.skill?.enabled ?? false;
  const mcpEnabled = !!config?.mcp;

  const items: ActivityItem[] = [
    { id: 'chat', icon: MessageSquare, label: 'Chat' },
    ...(skillsEnabled ? [{ id: 'skills' as const, icon: Blocks, label: 'Skills' }] : []),
    ...(mcpEnabled ? [{ id: 'mcp' as const, icon: Server, label: 'MCP' }] : []),
  ];

  return (
    <div className="w-12 border-r bg-muted/30 flex flex-col items-center py-2 gap-1 shrink-0">
      {items.map((item) => {
        const Icon = item.icon;
        const isActive = activeView === item.id && sidebarOpen;

        return (
          <Tooltip key={item.id}>
            <TooltipTrigger asChild>
              <button
                onClick={() => handleActivityBarClick(item.id)}
                className={cn(
                  'relative w-10 h-10 flex items-center justify-center rounded-lg transition-colors',
                  isActive
                    ? 'text-foreground bg-muted'
                    : 'text-muted-foreground hover:text-foreground hover:bg-muted/60'
                )}
              >
                {isActive && (
                  <div className="absolute left-0 top-1/2 -translate-y-1/2 w-0.5 h-5 bg-primary rounded-r" />
                )}
                <Icon className="h-5 w-5" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="right">
              {item.label}
            </TooltipContent>
          </Tooltip>
        );
      })}
    </div>
  );
}
