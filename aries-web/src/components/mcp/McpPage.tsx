import { useEffect } from 'react';
import { Server, RefreshCw } from 'lucide-react';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Button } from '@/components/ui/button';
import { useMcpStore } from '@/stores';
import { McpServerCard } from './McpServerCard';

export function McpPage() {
  const { servers, isLoading, togglingServer, serverErrors, fetchServers, toggleServer, updateApiKey } =
    useMcpStore();

  useEffect(() => {
    fetchServers();
  }, [fetchServers]);

  const enabledCount = servers.filter((s) => s.enable).length;

  return (
    <div className="h-full flex flex-col">
      {/* Header */}
      <div className="px-8 pt-8 pb-4">
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-2xl font-bold tracking-tight">MCP Servers</h1>
            <p className="text-base text-muted-foreground mt-1">
              Manage Model Context Protocol servers.
            </p>
          </div>
          <div className="flex items-center gap-3">
            {servers.length > 0 && (
              <span className="text-xs text-muted-foreground">
                {enabledCount}/{servers.length} enabled
              </span>
            )}
            <Button
              variant="outline"
              size="sm"
              onClick={fetchServers}
              disabled={isLoading}
            >
              <RefreshCw className={`h-3.5 w-3.5 mr-1.5 ${isLoading ? 'animate-spin' : ''}`} />
              刷新
            </Button>
          </div>
        </div>
      </div>

      {/* Content */}
      <ScrollArea className="flex-1 overflow-hidden">
        <div className="px-8 pb-8">
          {isLoading && servers.length === 0 ? (
            <div className="flex items-center justify-center py-20 text-muted-foreground">
              <RefreshCw className="h-5 w-5 animate-spin mr-2" />
              Loading...
            </div>
          ) : servers.length === 0 ? (
            <div className="flex flex-col items-center justify-center py-20 text-muted-foreground">
              <Server className="h-12 w-12 mb-3 opacity-30" />
              <p className="text-sm">No MCP servers configured</p>
            </div>
          ) : (
            <>
              <div className="text-xs text-foreground mb-4 font-semibold">
                {servers.length} server{servers.length !== 1 ? 's' : ''} configured
              </div>
              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                {servers.map((server, index) => (
                  <McpServerCard
                    key={`${server.name}-${index}`}
                    server={server}
                    isToggling={togglingServer === server.name}
                    toggleError={serverErrors[server.name]}
                    onToggle={(enable) => toggleServer(server.name, enable)}
                    onUpdateApiKey={(apiKey, apiKeyParam) =>
                      updateApiKey(server.name, apiKey, apiKeyParam)
                    }
                  />
                ))}
              </div>
            </>
          )}
        </div>
      </ScrollArea>
    </div>
  );
}

export default McpPage;
