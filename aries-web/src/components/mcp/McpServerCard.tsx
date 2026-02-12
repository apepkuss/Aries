import { useState } from 'react';
import { Server, Terminal, Globe, Wrench, Key, Loader2 } from 'lucide-react';
import { Switch } from '@/components/ui/switch';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog';
import type { SanitizedMcpToolServer } from '@/api/types';

interface McpServerCardProps {
  server: SanitizedMcpToolServer;
  isToggling: boolean;
  onToggle: (enable: boolean) => void;
  onUpdateApiKey: (apiKey: string, apiKeyParam?: string) => Promise<void>;
}

export function McpServerCard({ server, isToggling, onToggle, onUpdateApiKey }: McpServerCardProps) {
  const [apiKeyDialogOpen, setApiKeyDialogOpen] = useState(false);
  const [apiKeyInput, setApiKeyInput] = useState('');
  const [apiKeyParamInput, setApiKeyParamInput] = useState(server.api_key_param || '');
  const [isSaving, setIsSaving] = useState(false);

  const isStdio = server.transport === 'stdio';
  const TransportIcon = isStdio ? Terminal : Globe;

  const handleSaveApiKey = async () => {
    setIsSaving(true);
    try {
      await onUpdateApiKey(apiKeyInput, apiKeyParamInput || undefined);
      setApiKeyDialogOpen(false);
      setApiKeyInput('');
    } catch {
      // Error handled by store
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <div className="group rounded-xl border border-border/60 bg-card p-4 transition-all duration-200 hover:shadow-md hover:border-border">
      {/* Header */}
      <div className="flex items-start gap-3">
        <div className="shrink-0 h-9 w-9 rounded-lg bg-primary/10 flex items-center justify-center">
          <Server className="h-4.5 w-4.5 text-primary" />
        </div>
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <h3 className="text-sm font-semibold truncate">{server.name}</h3>
            <Badge variant="outline" className="text-[10px] px-1.5 py-0 h-5 shrink-0">
              <TransportIcon className="h-2.5 w-2.5 mr-1" />
              {server.transport}
            </Badge>
          </div>
          <p className="text-xs text-muted-foreground mt-0.5 truncate" title={server.url || server.command || ''}>
            {server.url || server.command || '-'}
          </p>
        </div>
        <Switch
          checked={server.enable}
          onCheckedChange={onToggle}
          disabled={isToggling}
        />
      </div>

      {/* Footer info */}
      <div className="mt-3 pt-3 border-t border-border/40 flex items-center gap-3">
        {/* Tools count */}
        <div className="flex items-center gap-1">
          <Wrench className="h-3 w-3 text-muted-foreground" />
          <span className="text-[11px] text-muted-foreground font-medium">
            {server.tools_count} tools
          </span>
        </div>

        {/* API Key status + button (only show when api_key_param is configured) */}
        {server.api_key_param && (
          <Dialog open={apiKeyDialogOpen} onOpenChange={setApiKeyDialogOpen}>
            <DialogTrigger asChild>
              <button className="flex items-center gap-1 text-[11px] font-medium hover:text-foreground transition-colors cursor-pointer">
                <Key className="h-3 w-3" />
                <span className={server.api_key_configured ? 'text-emerald-600 dark:text-emerald-400' : 'text-amber-600 dark:text-amber-400'}>
                  {server.api_key_configured ? 'API Key Set' : 'Set API Key'}
                </span>
              </button>
            </DialogTrigger>
            <DialogContent className="sm:max-w-md">
              <DialogHeader>
                <DialogTitle>API Key - {server.name}</DialogTitle>
                <DialogDescription>
                  Set the API key for this MCP server. The key will be appended as a query parameter.
                </DialogDescription>
              </DialogHeader>
              <div className="space-y-4 py-2">
                <div className="space-y-2">
                  <Label htmlFor="api-key">API Key</Label>
                  <Input
                    id="api-key"
                    type="password"
                    placeholder="Enter API key..."
                    value={apiKeyInput}
                    onChange={(e) => setApiKeyInput(e.target.value)}
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="api-key-param">Query Parameter Name</Label>
                  <Input
                    id="api-key-param"
                    placeholder="e.g., tavilyApiKey"
                    value={apiKeyParamInput}
                    onChange={(e) => setApiKeyParamInput(e.target.value)}
                  />
                  <p className="text-xs text-muted-foreground">
                    The URL parameter name used to pass the API key.
                  </p>
                </div>
              </div>
              <DialogFooter>
                <Button variant="outline" onClick={() => setApiKeyDialogOpen(false)}>
                  Cancel
                </Button>
                <Button onClick={handleSaveApiKey} disabled={isSaving || !apiKeyInput.trim()}>
                  {isSaving && <Loader2 className="h-3.5 w-3.5 mr-1.5 animate-spin" />}
                  Save
                </Button>
              </DialogFooter>
            </DialogContent>
          </Dialog>
        )}

        {/* Toggling indicator */}
        {isToggling && (
          <div className="flex items-center gap-1 ml-auto">
            <Loader2 className="h-3 w-3 animate-spin text-muted-foreground" />
            <span className="text-[11px] text-muted-foreground">
              {server.enable ? 'Disabling...' : 'Enabling...'}
            </span>
          </div>
        )}
      </div>
    </div>
  );
}
