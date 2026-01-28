import { useEffect } from 'react';
import { ChevronDown } from 'lucide-react';
import { useConfigStore } from '@/stores';

// Combined mode representing execution_mode + parallel_mode
type CombinedMode = 'plan' | 'subagent';

interface ModeOption {
  value: CombinedMode;
  label: string;
  description: string;
  // Config values to set
  execution_mode: 'direct' | 'subagent';
  parallel_mode: 'auto' | 'sequential' | 'manual';
}

const modeOptions: ModeOption[] = [
  {
    value: 'plan',
    label: 'Plan',
    description: '顺序执行',
    execution_mode: 'direct',
    parallel_mode: 'auto', // Not used in plan mode, but need a value
  },
  {
    value: 'subagent',
    label: 'Agent Swarm',
    description: '并行执行',
    execution_mode: 'subagent',
    parallel_mode: 'auto',
  },
];

// Helper to get combined mode from config
function getCombinedMode(
  executionMode: 'direct' | 'subagent' | undefined
): CombinedMode {
  if (executionMode === 'subagent') {
    return 'subagent';
  }
  return 'plan'; // default
}

export function ExecutionModeSelector() {
  const { config, fetchConfig, setPendingChange, saveChanges, isSaving } = useConfigStore();

  // Fetch config on mount if not loaded
  useEffect(() => {
    if (!config) {
      fetchConfig();
    }
  }, [config, fetchConfig]);

  const currentCombinedMode = getCombinedMode(config?.subagent?.execution_mode);

  const handleModeChange = async (e: React.ChangeEvent<HTMLSelectElement>) => {
    const combinedMode = e.target.value as CombinedMode;
    if (combinedMode === currentCombinedMode) return;

    const selectedOption = modeOptions.find((o) => o.value === combinedMode);
    if (!selectedOption) return;

    // Set both execution_mode and parallel_mode
    setPendingChange('subagent', 'execution_mode', selectedOption.execution_mode);
    setPendingChange('subagent', 'parallel_mode', selectedOption.parallel_mode);

    // Immediately save the change (hot-switch)
    await saveChanges();
  };

  // Don't render if subagent config is not available
  if (!config?.subagent) {
    return null;
  }

  const currentOption = modeOptions.find((o) => o.value === currentCombinedMode);

  return (
    <div className="flex items-center gap-2">
      <span className="text-xs text-muted-foreground">Mode:</span>
      <div className="relative">
        <select
          value={currentCombinedMode}
          onChange={handleModeChange}
          disabled={isSaving}
          className="appearance-none bg-muted/50 border border-border/50 rounded-md px-2.5 py-1 pr-7 text-xs font-medium cursor-pointer hover:bg-muted/80 focus:outline-none focus:ring-1 focus:ring-primary/50 disabled:opacity-50 disabled:cursor-not-allowed"
          title={currentOption?.description}
        >
          {modeOptions.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
        <ChevronDown className="absolute right-1.5 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground pointer-events-none" />
      </div>
      <span className="text-[10px] text-muted-foreground">
        ({currentOption?.description})
      </span>
    </div>
  );
}
