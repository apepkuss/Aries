import { useEffect, useRef, useState } from 'react';
import { AlertCircle, CheckCircle2 } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Switch } from '@/components/ui/switch';
import { useConfigStore } from '@/stores';

export function ServiceConfigForm() {
  const { config, pendingChanges, setPendingChange, urlTestState, urlTestError } = useConfigStore();

  // Track whether API Key input is enabled
  const [enableApiKey, setEnableApiKey] = useState(false);

  // Track the original URL to detect changes
  const originalUrlRef = useRef<string | undefined>(undefined);

  // Get current values (pending or saved)
  const savedUrl = config?.chat?.url ?? '';
  const chatUrl = pendingChanges.chat?.url ?? savedUrl;
  const chatApiKey = pendingChanges.chat?.api_key ?? '';
  const hasExistingApiKey = config?.chat?.api_key_configured ?? false;

  // Initialize original URL reference
  useEffect(() => {
    if (originalUrlRef.current === undefined && savedUrl) {
      originalUrlRef.current = savedUrl;
    }
  }, [savedUrl]);

  // Detect URL changes and reset API Key state
  useEffect(() => {
    // Only trigger when URL has been modified from the original saved value
    if (originalUrlRef.current !== undefined && chatUrl !== originalUrlRef.current) {
      // URL changed - disable API Key and mark for clearing
      setEnableApiKey(false);
      setPendingChange('chat', 'api_key', '');
    }
  }, [chatUrl, setPendingChange]);

  // Handle API Key switch toggle
  const handleApiKeyToggle = (checked: boolean) => {
    setEnableApiKey(checked);
    if (!checked) {
      // When disabled, mark API key for clearing
      setPendingChange('chat', 'api_key', '');
    }
  };

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

          {/* Test result feedback */}
          {urlTestState === 'failed' && urlTestError && (
            <div className="flex items-center gap-2 text-destructive text-sm">
              <AlertCircle className="h-4 w-4 shrink-0" />
              <span>{urlTestError}</span>
            </div>
          )}
          {urlTestState === 'passed' && (
            <div className="flex items-center gap-2 text-green-600 text-sm">
              <CheckCircle2 className="h-4 w-4 shrink-0" />
              <span>Connection test successful</span>
            </div>
          )}
        </div>

        <div className="space-y-2">
          <div className="flex items-center gap-3">
            <Switch
              id="enable-api-key"
              checked={enableApiKey}
              onCheckedChange={handleApiKeyToggle}
            />
            <Label htmlFor="enable-api-key" className="cursor-pointer">
              Enable API Key
            </Label>
          </div>

          <Input
            id="chat-api-key"
            type="password"
            placeholder={hasExistingApiKey && !enableApiKey ? '••••••••' : 'Enter API key'}
            value={chatApiKey}
            onChange={(e) => setPendingChange('chat', 'api_key', e.target.value)}
            disabled={!enableApiKey}
          />

          <p className="text-xs text-muted-foreground">
            {!enableApiKey
              ? hasExistingApiKey
                ? 'Enable the switch to update or clear the API key.'
                : 'Enable the switch if this service requires an API key.'
              : 'Enter the API key for authentication.'}
          </p>
        </div>

      </div>
    </div>
  );
}
