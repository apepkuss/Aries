import { useEffect, useState } from 'react';
import { ChevronDown } from 'lucide-react';
import { useConfigStore, useUIStore } from '@/stores';
import { fetchModels, type Model } from '@/api/models';

export function ModelSelector() {
  const [models, setModels] = useState<Model[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { config, setPendingChange, saveChanges, isSaving } = useConfigStore();
  const { privacyMode } = useUIStore();

  // Select config section based on privacy mode
  const serviceKey = privacyMode ? 'privacy_chat' : 'chat';
  const currentModel = config?.[serviceKey]?.model;
  const serviceUrl = config?.[serviceKey]?.url;

  // Fetch models when service URL or mode changes
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
        setModels(response.data);
      } catch (err) {
        console.error('Failed to fetch models:', err);
        setError('Failed to load models');
      } finally {
        setIsLoading(false);
      }
    };

    loadModels();
  }, [serviceUrl, serviceKey]);

  const handleModelChange = async (e: React.ChangeEvent<HTMLSelectElement>) => {
    const newModel = e.target.value;
    if (newModel === currentModel) return;

    setPendingChange(serviceKey, 'model', newModel);
    await saveChanges();
  };

  // Don't render if service not configured
  if (!serviceUrl) {
    return null;
  }

  // Don't render if loading or error
  if (isLoading) {
    return (
      <div className="flex items-center gap-2">
        <span className="text-xs text-muted-foreground">Loading...</span>
      </div>
    );
  }

  if (error || models.length === 0) {
    return null;
  }

  return (
    <div className="flex items-center gap-2">
      <div className="relative">
        <select
          value={currentModel || ''}
          onChange={handleModelChange}
          disabled={isSaving}
          className="appearance-none bg-muted/50 border border-border/50 rounded-md px-2.5 py-1 pr-7 text-xs font-medium cursor-pointer hover:bg-muted/80 focus:outline-none focus:ring-1 focus:ring-primary/50 disabled:opacity-50 disabled:cursor-not-allowed"
        >
          {models.map((model) => (
            <option key={model.id} value={model.id}>
              {model.id}
            </option>
          ))}
        </select>
        <ChevronDown className="absolute right-1.5 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground pointer-events-none" />
      </div>
    </div>
  );
}
