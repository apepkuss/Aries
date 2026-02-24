import { useEffect, useRef, useState } from 'react';
import { AlertCircle, CheckCircle2, ShieldCheck } from 'lucide-react';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Switch } from '@/components/ui/switch';
import { useServiceStore } from '@/stores';

export function ServiceConfigForm() {
  const {
    chat, privacyChat,
    pendingChat, pendingPrivacyChat,
    setPendingChat, setPendingPrivacyChat,
    chatTestState, chatTestError,
    privacyChatTestState, privacyChatTestError,
  } = useServiceStore();

  // Track whether API Key input is enabled
  const [enableApiKey, setEnableApiKey] = useState(false);
  const [enablePrivacyApiKey, setEnablePrivacyApiKey] = useState(false);

  // Track the original URLs to detect changes
  const originalUrlRef = useRef<string | undefined>(undefined);
  const originalPrivacyUrlRef = useRef<string | undefined>(undefined);

  // Get current values (pending or saved) - Chat Service
  const savedUrl = chat?.url ?? '';
  const chatUrl = pendingChat.url ?? savedUrl;
  const chatApiKey = pendingChat.apiKey ?? '';
  const hasExistingApiKey = !!(chat?.apiKey);
  const isChatConfigured = !!savedUrl;

  // Get current values (pending or saved) - Privacy Chat Service
  const savedPrivacyUrl = privacyChat?.url ?? '';
  const privacyChatUrl = pendingPrivacyChat.url ?? savedPrivacyUrl;
  const privacyChatApiKey = pendingPrivacyChat.apiKey ?? '';
  const hasExistingPrivacyApiKey = !!(privacyChat?.apiKey);
  const isPrivacyChatConfigured = !!savedPrivacyUrl;

  // Initialize original URL references
  useEffect(() => {
    if (originalUrlRef.current === undefined && savedUrl) {
      originalUrlRef.current = savedUrl;
    }
  }, [savedUrl]);

  useEffect(() => {
    if (originalPrivacyUrlRef.current === undefined && savedPrivacyUrl) {
      originalPrivacyUrlRef.current = savedPrivacyUrl;
    }
  }, [savedPrivacyUrl]);

  // Detect URL changes and reset API Key state
  useEffect(() => {
    if (originalUrlRef.current !== undefined && chatUrl !== originalUrlRef.current) {
      setEnableApiKey(false);
      setPendingChat('apiKey', '');
    }
  }, [chatUrl, setPendingChat]);

  useEffect(() => {
    if (originalPrivacyUrlRef.current !== undefined && privacyChatUrl !== originalPrivacyUrlRef.current) {
      setEnablePrivacyApiKey(false);
      setPendingPrivacyChat('apiKey', '');
    }
  }, [privacyChatUrl, setPendingPrivacyChat]);

  // Handle API Key switch toggle
  const handleApiKeyToggle = (checked: boolean) => {
    setEnableApiKey(checked);
    if (!checked) {
      setPendingChat('apiKey', '');
    }
  };

  const handlePrivacyApiKeyToggle = (checked: boolean) => {
    setEnablePrivacyApiKey(checked);
    if (!checked) {
      setPendingPrivacyChat('apiKey', '');
    }
  };

  return (
    <div className="space-y-6">
      {/* Chat Service */}
      <div className="space-y-4">
        <div className="flex items-center gap-2">
          <h4 className="font-medium">Chat Service</h4>
          <span
            className={`h-2 w-2 rounded-full shrink-0 ${
              isChatConfigured ? 'bg-green-500' : 'bg-muted-foreground/40'
            }`}
            title={isChatConfigured ? 'Configured' : 'Not configured'}
          />
        </div>

        <div className="space-y-2">
          <Label htmlFor="chat-url">
            URL <span className="text-destructive">*</span>
          </Label>
          <Input
            id="chat-url"
            type="url"
            placeholder="http://localhost:8080/v1"
            value={chatUrl}
            onChange={(e) => setPendingChat('url', e.target.value)}
          />
          <p className="text-xs text-muted-foreground">
            Chat completion service endpoint (OpenAI compatible)
          </p>

          {/* Test result feedback */}
          {chatTestState === 'failed' && chatTestError && (
            <div className="flex items-center gap-2 text-destructive text-sm">
              <AlertCircle className="h-4 w-4 shrink-0" />
              <span>{chatTestError}</span>
            </div>
          )}
          {chatTestState === 'passed' && (
            <div className="flex items-center gap-2 text-green-600 text-sm">
              <CheckCircle2 className="h-4 w-4 shrink-0" />
              <span>Connected successfully</span>
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
            onChange={(e) => setPendingChat('apiKey', e.target.value)}
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

      {/* Divider */}
      <div className="border-t border-border/60" />

      {/* Privacy Chat Service */}
      <div className="space-y-4">
        <div className="flex items-center gap-2">
          <ShieldCheck className="h-5 w-5 text-emerald-500 fill-emerald-500/20 shrink-0" />
          <div>
            <div className="flex items-center gap-2">
              <h4 className="font-medium">Privacy Chat Service</h4>
              <span
                className={`h-2 w-2 rounded-full shrink-0 ${
                  isPrivacyChatConfigured ? 'bg-green-500' : 'bg-muted-foreground/40'
                }`}
                title={isPrivacyChatConfigured ? 'Configured' : 'Not configured'}
              />
            </div>
            <p className="text-xs text-muted-foreground mt-0.5">
              Used when privacy mode is enabled. Typically a locally deployed model.
            </p>
          </div>
        </div>

        <div className="space-y-2">
          <Label htmlFor="privacy-chat-url">
            URL <span className="text-destructive">*</span>
          </Label>
          <Input
            id="privacy-chat-url"
            type="url"
            placeholder="http://localhost:8080/v1"
            value={privacyChatUrl}
            onChange={(e) => setPendingPrivacyChat('url', e.target.value)}
          />
          <p className="text-xs text-muted-foreground">
            Privacy chat completion service endpoint (OpenAI compatible)
          </p>

          {/* Test result feedback */}
          {privacyChatTestState === 'failed' && privacyChatTestError && (
            <div className="flex items-center gap-2 text-destructive text-sm">
              <AlertCircle className="h-4 w-4 shrink-0" />
              <span>{privacyChatTestError}</span>
            </div>
          )}
          {privacyChatTestState === 'passed' && (
            <div className="flex items-center gap-2 text-green-600 text-sm">
              <CheckCircle2 className="h-4 w-4 shrink-0" />
              <span>Connected successfully</span>
            </div>
          )}
        </div>

        <div className="space-y-2">
          <div className="flex items-center gap-3">
            <Switch
              id="enable-privacy-api-key"
              checked={enablePrivacyApiKey}
              onCheckedChange={handlePrivacyApiKeyToggle}
            />
            <Label htmlFor="enable-privacy-api-key" className="cursor-pointer">
              Enable API Key
            </Label>
          </div>

          <Input
            id="privacy-chat-api-key"
            type="password"
            placeholder={hasExistingPrivacyApiKey && !enablePrivacyApiKey ? '••••••••' : 'Enter API key'}
            value={privacyChatApiKey}
            onChange={(e) => setPendingPrivacyChat('apiKey', e.target.value)}
            disabled={!enablePrivacyApiKey}
          />

          <p className="text-xs text-muted-foreground">
            {!enablePrivacyApiKey
              ? hasExistingPrivacyApiKey
                ? 'Enable the switch to update or clear the API key.'
                : 'Enable the switch if this service requires an API key.'
              : 'Enter the API key for authentication.'}
          </p>
        </div>
      </div>
    </div>
  );
}
