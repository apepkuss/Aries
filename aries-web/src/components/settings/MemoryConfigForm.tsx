import { useState, useCallback } from 'react';
import { Label } from '@/components/ui/label';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { ChevronDown, RefreshCw, AlertCircle } from 'lucide-react';
import { useConfigStore } from '@/stores';
import { registerServer } from '@/api/admin';
import { fetchModels } from '@/api/models';

const CONTEXT_SIZE_OPTIONS = [
  { label: 'Disabled', value: 0 },
  { label: '8K', value: 8192 },
  { label: '32K', value: 32768 },
  { label: '64K', value: 65536 },
  { label: '128K', value: 128000 },
  { label: '200K', value: 200000 },
];

function formatContextSize(size: number): string {
  if (size === 0) return 'Disabled';
  const preset = CONTEXT_SIZE_OPTIONS.find((o) => o.value === size);
  if (preset) return preset.label;
  if (size >= 1000) return `${(size / 1000).toFixed(size % 1000 === 0 ? 0 : 1)}K`;
  return String(size);
}

export function MemoryConfigForm() {
  const { config, pendingChanges, setPendingChange } = useConfigStore();

  // Track whether API Key input is enabled for embedding
  const [enableEmbeddingApiKey, setEnableEmbeddingApiKey] = useState(false);

  // Embedding model fetch state
  const [availableModels, setAvailableModels] = useState<string[]>([]);
  const [fetchingModels, setFetchingModels] = useState(false);
  const [fetchError, setFetchError] = useState<string | null>(null);

  // Model context size (from chat config)
  const modelContextSize =
    pendingChanges.chat?.model_context_size ?? config?.chat?.model_context_size ?? 128000;

  // Checkpoint token ratio (from lantai auto memory config)
  const checkpointTokenRatio =
    pendingChanges.lantai_auto_memory?.checkpoint_token_ratio ??
    config?.lantai_auto_memory?.checkpoint_token_ratio ??
    0.75;

  // Embedding config
  const embeddingUrl =
    (pendingChanges.embedding?.url as string | undefined) ?? config?.embedding?.url ?? '';
  const embeddingApiKey = (pendingChanges.embedding?.api_key as string | undefined) ?? '';
  const hasExistingEmbeddingApiKey = !!(config?.embedding?.api_key_configured);
  const isEmbeddingConfigured = !!config?.embedding?.url;
  const embeddingModel =
    (pendingChanges.lantai_auto_memory?.embedding_model as string | undefined) ??
    (isEmbeddingConfigured ? config?.lantai_auto_memory?.embedding_model : undefined) ??
    '';
  const embeddingDimensions =
    (pendingChanges.lantai_auto_memory?.embedding_dimensions as number | undefined) ??
    (isEmbeddingConfigured ? config?.lantai_auto_memory?.embedding_dimensions : undefined) ??
    '';
  const embeddingBatchSize =
    (pendingChanges.lantai_auto_memory?.embedding_batch_size as number | undefined) ??
    (isEmbeddingConfigured ? config?.lantai_auto_memory?.embedding_batch_size : undefined) ??
    '';

  // Handle embedding API Key switch toggle
  const handleEmbeddingApiKeyToggle = (checked: boolean) => {
    setEnableEmbeddingApiKey(checked);
    if (!checked) {
      setPendingChange('embedding', 'api_key', '');
    }
  };

  // Fetch available models by registering the embedding service and querying models
  const handleFetchModels = useCallback(async () => {
    // Use pending URL or saved URL
    const url = (pendingChanges.embedding?.url as string | undefined) ?? config?.embedding?.url;
    if (!url) {
      setFetchError('Please enter a URL first');
      return;
    }

    setFetchingModels(true);
    setFetchError(null);

    try {
      // Register embedding service via admin API
      const apiKey = enableEmbeddingApiKey
        ? (pendingChanges.embedding?.api_key as string | undefined)
        : undefined;

      await registerServer({
        url,
        kind: 'embeddings',
        ...(apiKey ? { api_key: apiKey } : {}),
      });

      // Fetch models for the registered embeddings service
      const modelsResponse = await fetchModels('embeddings');
      const modelIds = [...new Set(modelsResponse.data.map((m) => m.id))];

      if (modelIds.length > 0) {
        setAvailableModels(modelIds);
      } else {
        setFetchError('No models found at this endpoint');
        setAvailableModels([]);
      }
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Failed to connect to service';
      setFetchError(message);
      setAvailableModels([]);
    } finally {
      setFetchingModels(false);
    }
  }, [pendingChanges, config, enableEmbeddingApiKey]);

  return (
    <div className="space-y-6">
      {/* Memory Settings */}
      <div className="space-y-4">
        <h4 className="font-medium">Memory Settings</h4>

        <div className="space-y-2">
          <Label>Model Context Size</Label>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="outline" className="w-full justify-between">
                {formatContextSize(modelContextSize)}
                <ChevronDown className="h-4 w-4 ml-2" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent className="w-full">
              {CONTEXT_SIZE_OPTIONS.map((option) => (
                <DropdownMenuItem
                  key={option.value}
                  onClick={() => setPendingChange('chat', 'model_context_size', option.value)}
                >
                  {option.label} {option.value > 0 && `(${option.value.toLocaleString()} tokens)`}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
          <p className="text-xs text-muted-foreground">
            Model context window size. Used for memory checkpoint triggers. Set to Disabled to turn off token-aware features.
          </p>
        </div>

        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <Label htmlFor="checkpoint-ratio">Checkpoint Token Ratio</Label>
            <span className="text-sm font-mono text-muted-foreground">
              {checkpointTokenRatio.toFixed(2)}
            </span>
          </div>
          <input
            id="checkpoint-ratio"
            type="range"
            min={0.5}
            max={0.95}
            step={0.05}
            value={checkpointTokenRatio}
            onChange={(e) => {
              const value = parseFloat(e.target.value);
              setPendingChange('lantai_auto_memory', 'checkpoint_token_ratio', value);
            }}
            className="w-full h-2 bg-muted rounded-lg appearance-none cursor-pointer accent-primary"
          />
          <p className="text-xs text-muted-foreground">
            Trigger memory checkpoint when prompt tokens reach this ratio of context size (0.50 ~ 0.95)
          </p>
        </div>
      </div>

      {/* Divider */}
      <div className="border-t border-border/60" />

      {/* Embedding Service */}
      <div className="space-y-4">
        <div className="flex items-center gap-2">
          <h4 className="font-medium">Embedding Service</h4>
          <span
            className={`h-2 w-2 rounded-full shrink-0 ${
              isEmbeddingConfigured ? 'bg-green-500' : 'bg-muted-foreground/40'
            }`}
            title={isEmbeddingConfigured ? 'Configured' : 'Not configured'}
          />
        </div>
        <p className="text-xs text-muted-foreground -mt-2">
          Optional. Enables vector semantic search for knowledge base. Without this, only BM25 text search is used.
        </p>

        <div className="space-y-2">
          <Label htmlFor="embedding-url">URL</Label>
          <Input
            id="embedding-url"
            type="url"
            placeholder="http://localhost:8080/v1"
            value={embeddingUrl}
            onChange={(e) => setPendingChange('embedding', 'url', e.target.value)}
          />
          <p className="text-xs text-muted-foreground">
            Embedding service endpoint (OpenAI compatible)
          </p>
        </div>

        <div className="space-y-2">
          <div className="flex items-center gap-3">
            <Switch
              id="enable-embedding-api-key"
              checked={enableEmbeddingApiKey}
              onCheckedChange={handleEmbeddingApiKeyToggle}
            />
            <Label htmlFor="enable-embedding-api-key" className="cursor-pointer">
              Enable API Key
            </Label>
          </div>

          <Input
            id="embedding-api-key"
            type="password"
            placeholder={hasExistingEmbeddingApiKey && !enableEmbeddingApiKey ? '••••••••' : 'Enter API key'}
            value={embeddingApiKey}
            onChange={(e) => setPendingChange('embedding', 'api_key', e.target.value)}
            disabled={!enableEmbeddingApiKey}
          />

          <p className="text-xs text-muted-foreground">
            {!enableEmbeddingApiKey
              ? hasExistingEmbeddingApiKey
                ? 'Enable the switch to update or clear the API key.'
                : 'Enable the switch if this service requires an API key.'
              : 'Enter the API key for authentication.'}
          </p>
        </div>

        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <Label>Model Name</Label>
            <Button
              variant="ghost"
              size="sm"
              className="h-7 px-2 text-xs"
              onClick={handleFetchModels}
              disabled={fetchingModels || !embeddingUrl}
            >
              <RefreshCw className={`h-3 w-3 mr-1 ${fetchingModels ? 'animate-spin' : ''}`} />
              {fetchingModels ? 'Fetching...' : 'Fetch Models'}
            </Button>
          </div>

          {availableModels.length > 0 ? (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="outline" className="w-full justify-between">
                  {embeddingModel || 'Select a model'}
                  <ChevronDown className="h-4 w-4 ml-2" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent className="w-full max-h-60 overflow-y-auto">
                {availableModels.map((model) => (
                  <DropdownMenuItem
                    key={model}
                    onClick={() => setPendingChange('lantai_auto_memory', 'embedding_model', model)}
                  >
                    {model}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          ) : (
            <Input
              id="embedding-model"
              type="text"
              placeholder="Click 'Fetch Models' to load available models"
              value={embeddingModel}
              onChange={(e) => setPendingChange('lantai_auto_memory', 'embedding_model', e.target.value)}
            />
          )}

          {fetchError && (
            <p className="text-xs text-destructive flex items-center gap-1">
              <AlertCircle className="h-3 w-3 shrink-0" />
              {fetchError}
            </p>
          )}

          <p className="text-xs text-muted-foreground">
            {availableModels.length > 0
              ? 'Select an embedding model from the list above.'
              : 'Enter a URL above and click "Fetch Models" to load available models, or type a model name manually.'}
          </p>
        </div>

        <div className="grid grid-cols-2 gap-4">
          <div className="space-y-2">
            <Label htmlFor="embedding-dimensions">Dimensions</Label>
            <Input
              id="embedding-dimensions"
              type="number"
              placeholder="e.g. 768, 1536"
              value={embeddingDimensions}
              onChange={(e) => {
                const val = e.target.value === '' ? undefined : parseInt(e.target.value, 10);
                if (val === undefined || !isNaN(val)) {
                  setPendingChange('lantai_auto_memory', 'embedding_dimensions', val);
                }
              }}
              min={1}
              max={8192}
            />
            <p className="text-xs text-muted-foreground">
              Vector dimensions of the embedding model
            </p>
          </div>

          <div className="space-y-2">
            <Label htmlFor="embedding-batch-size">Batch Size</Label>
            <Input
              id="embedding-batch-size"
              type="number"
              placeholder="e.g. 32"
              value={embeddingBatchSize}
              onChange={(e) => {
                const val = e.target.value === '' ? undefined : parseInt(e.target.value, 10);
                if (val === undefined || !isNaN(val)) {
                  setPendingChange('lantai_auto_memory', 'embedding_batch_size', val);
                }
              }}
              min={1}
              max={1024}
            />
            <p className="text-xs text-muted-foreground">
              Number of texts to embed per API request
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}
