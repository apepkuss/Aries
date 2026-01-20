import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Badge } from '@/components/ui/badge';
import { useConfigStore } from '@/stores';

export function ServiceConfigForm() {
  const { config, pendingChanges, setPendingChange } = useConfigStore();

  // Get current values (pending or saved)
  const chatUrl = pendingChanges.chat?.url ?? config?.chat?.url ?? '';
  const chatModel = pendingChanges.chat?.model ?? config?.chat?.model ?? '';
  const chatApiKey = pendingChanges.chat?.api_key ?? '';

  return (
    <div className="space-y-6">
      {/* Chat Service */}
      <div className="space-y-4">
        <h4 className="font-medium">Chat Service</h4>

        <div className="space-y-2">
          <Label htmlFor="chat-url">
            URL <span className="text-destructive">*</span>
          </Label>
          <Input
            id="chat-url"
            type="url"
            placeholder="http://localhost:8080/v1"
            value={chatUrl}
            onChange={(e) => setPendingChange('chat', 'url', e.target.value)}
          />
          <p className="text-xs text-muted-foreground">
            Chat completion service endpoint (OpenAI compatible)
          </p>
        </div>

        <div className="space-y-2">
          <div className="flex items-center gap-2">
            <Label htmlFor="chat-api-key">API Key</Label>
            {config?.chat?.api_key_configured && (
              <Badge variant="secondary" className="text-xs">
                Configured
              </Badge>
            )}
          </div>
          <Input
            id="chat-api-key"
            type="password"
            placeholder={config?.chat?.api_key_configured ? '••••••••' : 'Enter API key'}
            value={chatApiKey}
            onChange={(e) => setPendingChange('chat', 'api_key', e.target.value)}
          />
          <p className="text-xs text-muted-foreground">
            Leave empty to keep current key. Changing URL or API key will reconnect the service.
          </p>
        </div>

        <div className="space-y-2">
          <Label htmlFor="chat-model">
            Model <span className="text-destructive">*</span>
          </Label>
          <Input
            id="chat-model"
            type="text"
            placeholder="gpt-4o, claude-3-5-sonnet, etc."
            value={chatModel}
            onChange={(e) => setPendingChange('chat', 'model', e.target.value)}
          />
          <p className="text-xs text-muted-foreground">
            Model name to use for chat completions
          </p>
        </div>
      </div>
    </div>
  );
}
