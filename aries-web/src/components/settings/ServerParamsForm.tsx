import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { useConfigStore } from '@/stores';

export function ServerParamsForm() {
  const { config, pendingChanges, setPendingChange } = useConfigStore();

  // Helper to get current server value
  const getValue = (field: keyof NonNullable<typeof config>['server']) => {
    const pending = pendingChanges.server?.[field as keyof typeof pendingChanges.server];
    if (pending !== undefined) return pending;
    return config?.server?.[field] ?? '';
  };

  // Helper to get current execution mode
  const getExecutionMode = (): 'direct' | 'subagent' => {
    const pending = pendingChanges.subagent?.execution_mode;
    if (pending !== undefined) return pending;
    return config?.subagent?.execution_mode ?? 'direct';
  };

  const fields = [
    {
      id: 'max_tools_per_iteration',
      label: 'Max Tools Per Iteration',
      description: 'Maximum number of tools that can be called in a single iteration',
      min: 1,
      max: 100,
    },
    {
      id: 'tool_call_max_retries',
      label: 'Tool Call Max Retries',
      description: 'Maximum retry attempts for failed tool calls',
      min: 0,
      max: 10,
    },
    {
      id: 'tool_call_retry_delay_ms',
      label: 'Tool Call Retry Delay (ms)',
      description: 'Delay between tool call retries in milliseconds',
      min: 100,
      max: 10000,
    },
    {
      id: 'max_plan_subtasks',
      label: 'Max Plan Subtasks',
      description: 'Maximum number of subtasks in Plan mode',
      min: 1,
      max: 50,
    },
    {
      id: 'plan_timeout_secs',
      label: 'Plan Timeout (seconds)',
      description: 'Timeout for Plan mode execution',
      min: 60,
      max: 3600,
    },
    {
      id: 'subtask_max_retries',
      label: 'Subtask Max Retries',
      description: 'Maximum retry attempts for failed subtasks',
      min: 0,
      max: 10,
    },
    {
      id: 'subtask_react_max_iterations',
      label: 'Subtask ReAct Max Iterations',
      description: 'Maximum ReAct iterations per subtask',
      min: 1,
      max: 20,
    },
    {
      id: 'subtask_react_timeout_secs',
      label: 'Subtask ReAct Timeout (seconds)',
      description: 'Timeout for each subtask ReAct execution',
      min: 10,
      max: 600,
    },
  ] as const;

  return (
    <div className="space-y-6">
      {/* Execution Mode Selection */}
      {config?.subagent && (
        <div className="space-y-4">
          <h4 className="font-medium">Execution Mode</h4>
          <div className="space-y-2">
            <Label htmlFor="execution_mode">Task Execution Mode</Label>
            <select
              id="execution_mode"
              className="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
              value={getExecutionMode()}
              onChange={(e) => {
                setPendingChange('subagent', 'execution_mode', e.target.value as 'direct' | 'subagent');
              }}
            >
              <option value="direct">Plan Mode (Direct Execution)</option>
              <option value="subagent">Sub-Agent Mode (Parallel Execution)</option>
            </select>
            <p className="text-xs text-muted-foreground">
              {getExecutionMode() === 'direct'
                ? 'Tasks are executed sequentially in a single ReAct loop'
                : 'Tasks are delegated to Sub-Agents for parallel autonomous execution'}
            </p>
          </div>
        </div>
      )}

      {/* Server Parameters */}
      <div className="space-y-4">
        <h4 className="font-medium">Server Parameters</h4>

        <div className="grid gap-4 sm:grid-cols-2">
          {fields.map((field) => (
            <div key={field.id} className="space-y-2">
              <Label htmlFor={field.id}>{field.label}</Label>
              <Input
                id={field.id}
                type="number"
                min={field.min}
                max={field.max}
                value={getValue(field.id)}
                onChange={(e) => {
                  const value = parseInt(e.target.value, 10);
                  if (!isNaN(value)) {
                    setPendingChange('server', field.id, value);
                  }
                }}
              />
              <p className="text-xs text-muted-foreground">{field.description}</p>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
