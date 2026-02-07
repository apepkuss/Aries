import { useState } from 'react';
import { Loader2, AlertCircle, CheckCircle2, Rocket } from 'lucide-react';
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

  const [url, setUrl] = useState('');
  const [apiKey, setApiKey] = useState('');
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
    store.setPendingChat('url', trimmedUrl);
    if (apiKey.trim()) {
      store.setPendingChat('apiKey', apiKey.trim());
    }

    const success = await store.connectChat();
    if (success) {
      setStatus('success');
      // chat becomes non-null → component unmounts on next render
    } else {
      setStatus('error');
      setError(
        useServiceStore.getState().chatTestError || 'Failed to connect'
      );
    }
  };

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
            Connect a chat service to get started. You can update these settings
            at any time.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-2">
          {/* Service URL */}
          <div className="space-y-2">
            <Label htmlFor="setup-url">
              Chat Service URL <span className="text-destructive">*</span>
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
              disabled={status === 'connecting'}
              autoFocus
            />
            <p className="text-xs text-muted-foreground">
              OpenAI-compatible chat completion endpoint
            </p>
          </div>

          {/* API Key */}
          <div className="space-y-2">
            <Label htmlFor="setup-api-key">API Key</Label>
            <Input
              id="setup-api-key"
              type="password"
              placeholder="sk-... (optional)"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              disabled={status === 'connecting'}
            />
            <p className="text-xs text-muted-foreground">
              Required if the service needs authentication
            </p>
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
            disabled={!url.trim() || status === 'connecting'}
            className="w-full"
          >
            {status === 'connecting' && (
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            )}
            {status === 'connecting' ? 'Connecting...' : 'Connect'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
