import { useEffect, useState } from 'react';
import { ChevronDown, Cloud, ShieldCheck } from 'lucide-react';
import { useServiceStore } from '@/stores';
import { fetchModels, type Model } from '@/api/models';

interface SingleModelSelectorProps {
  serviceKey: 'chat' | 'privacy_chat';
  serviceUrl: string | undefined;
  currentModel: string | undefined;
  onModelChange: (model: string) => void;
  icon: React.ReactNode;
  title: string;
}

function SingleModelSelector({
  serviceKey,
  serviceUrl,
  currentModel,
  onModelChange,
  icon,
  title,
}: SingleModelSelectorProps) {
  const [models, setModels] = useState<Model[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Fetch models when service URL changes
  useEffect(() => {
    if (!serviceUrl) {
      setModels([]);
      return;
    }

    const loadModels = async () => {
      try {
        setIsLoading(true);
        setError(null);
        const response = await fetchModels(serviceKey);
        // Deduplicate by model id (backend may return duplicates
        // when multiple servers are registered for the same service)
        const seen = new Set<string>();
        const unique = response.data.filter((m) => {
          if (seen.has(m.id)) return false;
          seen.add(m.id);
          return true;
        });
        setModels(unique);
      } catch (err) {
        console.error(`Failed to fetch models for ${serviceKey}:`, err);
        setError('Failed to load models');
      } finally {
        setIsLoading(false);
      }
    };

    loadModels();
  }, [serviceUrl, serviceKey]);

  const handleChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    const newModel = e.target.value;
    if (newModel === currentModel) return;
    onModelChange(newModel);
  };

  // Don't render if service not configured
  if (!serviceUrl) {
    return null;
  }

  // Show loading state
  if (isLoading) {
    return (
      <div className="relative" title={title}>
        <div className="absolute left-1.5 top-1/2 -translate-y-1/2 pointer-events-none">
          {icon}
        </div>
        <div className="bg-muted/50 border border-border/50 rounded-md pl-6 pr-5 py-1 text-xs text-muted-foreground">
          ...
        </div>
      </div>
    );
  }

  // Don't render if error or no models
  if (error || models.length === 0) {
    return null;
  }

  return (
    <div className="relative" title={title}>
      {/* Icon inside the select box (left) */}
      <div className="absolute left-1.5 top-1/2 -translate-y-1/2 pointer-events-none">
        {icon}
      </div>
      <select
        value={currentModel || ''}
        onChange={handleChange}
        className="appearance-none bg-muted/50 border border-border/50 rounded-md pl-6 pr-5 py-1 text-xs font-medium cursor-pointer hover:bg-muted/80 focus:outline-none focus:ring-1 focus:ring-primary/50"
      >
        {models.map((model) => (
          <option key={model.id} value={model.id}>
            {model.id}
          </option>
        ))}
      </select>
      {/* Chevron icon (right) */}
      <ChevronDown className="absolute right-1 top-1/2 -translate-y-1/2 h-3 w-3 text-muted-foreground pointer-events-none" />
    </div>
  );
}

export function ModelSelector() {
  const { chat, privacyChat, setChatModel, setPrivacyChatModel } = useServiceStore();

  const hasChatService = !!chat?.url;
  const hasPrivacyChatService = !!privacyChat?.url;

  // Don't render if no services configured
  if (!hasChatService && !hasPrivacyChatService) {
    return null;
  }

  return (
    <div className="flex items-center gap-2">
      {/* Chat Service Model Selector */}
      <SingleModelSelector
        serviceKey="chat"
        serviceUrl={chat?.url}
        currentModel={chat?.model}
        onModelChange={setChatModel}
        icon={<Cloud className="h-3.5 w-3.5 text-muted-foreground" />}
        title="Cloud Model (for normal chat)"
      />

      {/* Privacy Chat Service Model Selector */}
      <SingleModelSelector
        serviceKey="privacy_chat"
        serviceUrl={privacyChat?.url}
        currentModel={privacyChat?.model}
        onModelChange={setPrivacyChatModel}
        icon={<ShieldCheck className="h-3.5 w-3.5 text-emerald-500" />}
        title="Privacy Model (for sensitive content)"
      />
    </div>
  );
}
