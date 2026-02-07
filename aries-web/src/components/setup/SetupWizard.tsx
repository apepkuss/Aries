import { useState } from 'react';
import {
  Loader2,
  AlertCircle,
  CheckCircle2,
  Rocket,
  ShieldCheck,
} from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Button } from '@/components/ui/button';
import { useServiceStore } from '@/stores';

type Status = 'idle' | 'connecting' | 'success' | 'error';

export function SetupWizard() {
  const chat = useServiceStore((s) => s.chat);

  // Chat service fields
  const [url, setUrl] = useState('');
  const [apiKey, setApiKey] = useState('');

  // Privacy chat service fields
  const [privacyUrl, setPrivacyUrl] = useState('');
  const [privacyApiKey, setPrivacyApiKey] = useState('');

  const [status, setStatus] = useState<Status>('idle');
  const [error, setError] = useState('');

  // Show wizard only when no chat service has ever been configured
  if (chat !== null) return null;

  const handleConnect = async () => {
    const trimmedUrl = url.trim();
    if (!trimmedUrl) return;

    setStatus('connecting');
    setError('');

    const store = useServiceStore.getState();

    // Register chat service
    store.setPendingChat('url', trimmedUrl);
    if (apiKey.trim()) {
      store.setPendingChat('apiKey', apiKey.trim());
    }

    const chatOk = await store.connectChat();
    if (!chatOk) {
      setStatus('error');
      setError(
        useServiceStore.getState().chatTestError ||
          'Failed to connect chat service'
      );
      return;
    }

    // Register privacy chat service (if provided)
    const trimmedPrivacyUrl = privacyUrl.trim();
    if (trimmedPrivacyUrl) {
      store.setPendingPrivacyChat('url', trimmedPrivacyUrl);
      if (privacyApiKey.trim()) {
        store.setPendingPrivacyChat('apiKey', privacyApiKey.trim());
      }

      const privacyOk = await store.connectPrivacyChat();
      if (!privacyOk) {
        setStatus('error');
        setError(
          useServiceStore.getState().privacyChatTestError ||
            'Failed to connect privacy chat service'
        );
        return;
      }
    }

    setStatus('success');
    // chat becomes non-null → component unmounts on next render
  };

  const connecting = status === 'connecting';

  return (
    <Dialog open>
      <DialogContent
        className="sm:max-w-md"
        showCloseButton={false}
        onPointerDownOutside={(e) => e.preventDefault()}
        onEscapeKeyDown={(e) => e.preventDefault()}
      >
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Rocket className="h-5 w-5" />
            Welcome to Aries
          </DialogTitle>
          <DialogDescription>
            Connect your services to get started. You can update these settings
            at any time.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-2 max-h-[60vh] overflow-y-auto scrollbar-overlay">
          {/* ── Chat Service ── */}
          <div className="space-y-3">
            <h4 className="text-sm font-medium">Chat Service</h4>

            <div className="space-y-2">
              <Label htmlFor="setup-url">
                URL <span className="text-destructive">*</span>
              </Label>
              <Input
                id="setup-url"
                type="url"
                placeholder="https://api.openai.com/v1"
                value={url}
                onChange={(e) => {
                  setUrl(e.target.value);
                  if (status === 'error') {
                    setStatus('idle');
                    setError('');
                  }
                }}
                disabled={connecting}
                autoFocus
              />
              <p className="text-xs text-muted-foreground">
                OpenAI-compatible chat completion endpoint
              </p>
            </div>

            <div className="space-y-2">
              <Label htmlFor="setup-api-key">API Key</Label>
              <Input
                id="setup-api-key"
                type="password"
                placeholder="sk-... (optional)"
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                disabled={connecting}
              />
            </div>
          </div>

          {/* Divider */}
          <div className="border-t border-border/60" />

          {/* ── Privacy Chat Service ── */}
          <div className="space-y-3">
            <div className="flex items-center gap-2">
              <ShieldCheck className="h-4 w-4 text-emerald-500 fill-emerald-500/20 shrink-0" />
              <h4 className="text-sm font-medium">Privacy Chat Service</h4>
              <span className="text-xs text-muted-foreground">(optional)</span>
            </div>

            <div className="space-y-2">
              <Label htmlFor="setup-privacy-url">URL</Label>
              <Input
                id="setup-privacy-url"
                type="url"
                placeholder="http://localhost:8080/v1"
                value={privacyUrl}
                onChange={(e) => {
                  setPrivacyUrl(e.target.value);
                  if (status === 'error') {
                    setStatus('idle');
                    setError('');
                  }
                }}
                disabled={connecting}
              />
              <p className="text-xs text-muted-foreground">
                Used when privacy mode is enabled. Typically a locally deployed
                model.
              </p>
            </div>

            <div className="space-y-2">
              <Label htmlFor="setup-privacy-api-key">API Key</Label>
              <Input
                id="setup-privacy-api-key"
                type="password"
                placeholder="(optional)"
                value={privacyApiKey}
                onChange={(e) => setPrivacyApiKey(e.target.value)}
                disabled={connecting}
              />
            </div>
          </div>

          {/* Feedback */}
          {status === 'error' && error && (
            <div className="flex items-center gap-2 text-destructive text-sm">
              <AlertCircle className="h-4 w-4 shrink-0" />
              <span>{error}</span>
            </div>
          )}
          {status === 'success' && (
            <div className="flex items-center gap-2 text-green-600 text-sm">
              <CheckCircle2 className="h-4 w-4 shrink-0" />
              <span>Connected successfully</span>
            </div>
          )}
        </div>

        <DialogFooter>
          <Button
            onClick={handleConnect}
            disabled={!url.trim() || connecting}
            className="w-full"
          >
            {connecting && (
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            )}
            {connecting ? 'Connecting...' : 'Connect'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
