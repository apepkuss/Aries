import { Settings, Moon, Sun, Monitor, SquarePen } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { useUIStore, useChatStore, useServiceStore } from '@/stores';
import { toast } from 'sonner';

export function Header() {
  const { theme, setTheme, setSettingsOpen } = useUIStore();
  const { clearMessages } = useChatStore();
  const { chat } = useServiceStore();

  const isChatConfigured = !!chat?.url;
  const electronAPI = (window as unknown as Record<string, unknown>)?.electronAPI as { platform: string } | undefined;
  const isElectronMac = electronAPI?.platform === 'darwin';

  return (
    <header className={`h-14 border-b bg-background sticky top-0 z-50 flex items-center justify-between px-4 electron-drag ${isElectronMac ? 'pl-24' : ''}`}>
      <div className="flex items-center gap-3 electron-no-drag">
        <div className="flex items-center border border-border/60 rounded-lg overflow-hidden">
          <button
            onClick={() => {
              if (!isChatConfigured) {
                toast.info('请先在设置中配置 Chat Service');
                setSettingsOpen(true);
                return;
              }
              clearMessages();
            }}
            className={`p-2 transition-colors ${isChatConfigured ? 'hover:bg-muted/60' : 'opacity-40 cursor-not-allowed'}`}
            title={isChatConfigured ? 'New chat' : 'Chat service not configured'}
          >
            <SquarePen className="h-[18px] w-[18px] text-foreground/80" />
          </button>
        </div>
      </div>

      <div className="flex items-center gap-2 electron-no-drag">
        {/* Theme switcher */}
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="icon">
              {theme === 'light' && <Sun className="h-5 w-5" />}
              {theme === 'dark' && <Moon className="h-5 w-5" />}
              {theme === 'system' && <Monitor className="h-5 w-5" />}
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem onClick={() => setTheme('light')}>
              <Sun className="mr-2 h-4 w-4" />
              Light
            </DropdownMenuItem>
            <DropdownMenuItem onClick={() => setTheme('dark')}>
              <Moon className="mr-2 h-4 w-4" />
              Dark
            </DropdownMenuItem>
            <DropdownMenuItem onClick={() => setTheme('system')}>
              <Monitor className="mr-2 h-4 w-4" />
              System
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>

        {/* Settings button */}
        <Button variant="ghost" size="icon" onClick={() => setSettingsOpen(true)}>
          <Settings className="h-5 w-5" />
        </Button>
      </div>
    </header>
  );
}
