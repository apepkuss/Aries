import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Switch } from '@/components/ui/switch';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Button } from '@/components/ui/button';
import { ChevronDown } from 'lucide-react';
import { useConfigStore } from '@/stores';

export function MemoryConfigForm() {
  const { config, pendingChanges, setPendingChange } = useConfigStore();

  // Get current values
  const autoSummarize =
    pendingChanges.memory?.auto_summarize ?? config?.memory?.auto_summarize ?? false;
  const summarizationStrategy =
    pendingChanges.memory?.summarization_strategy ??
    config?.memory?.summarization_strategy ??
    'Incremental';
  const summarizeThreshold =
    pendingChanges.memory?.summarize_threshold ?? config?.memory?.summarize_threshold ?? 10;
  const maxStoredMessages =
    pendingChanges.memory?.max_stored_messages ?? config?.memory?.max_stored_messages ?? 100;

  // RAG enable
  const ragEnable = pendingChanges.rag?.enable ?? config?.rag?.enable ?? false;

  return (
    <div className="space-y-6">
      {/* Memory Settings */}
      <div className="space-y-4">
        <h4 className="font-medium">Memory Settings</h4>

        <div className="flex items-center justify-between">
          <div className="space-y-0.5">
            <Label>Auto Summarize</Label>
            <p className="text-xs text-muted-foreground">
              Automatically summarize conversations when threshold is reached
            </p>
          </div>
          <Switch
            checked={autoSummarize}
            onCheckedChange={(checked) => setPendingChange('memory', 'auto_summarize', checked)}
          />
        </div>

        <div className="space-y-2">
          <Label>Summarization Strategy</Label>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="outline" className="w-full justify-between">
                {summarizationStrategy}
                <ChevronDown className="h-4 w-4 ml-2" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent className="w-full">
              <DropdownMenuItem
                onClick={() => setPendingChange('memory', 'summarization_strategy', 'Incremental')}
              >
                Incremental
              </DropdownMenuItem>
              <DropdownMenuItem
                onClick={() => setPendingChange('memory', 'summarization_strategy', 'FullHistory')}
              >
                FullHistory
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
          <p className="text-xs text-muted-foreground">
            Incremental: Only summarize new messages. FullHistory: Re-summarize all messages.
          </p>
        </div>

        <div className="space-y-2">
          <Label htmlFor="summarize-threshold">Summarize Threshold</Label>
          <Input
            id="summarize-threshold"
            type="number"
            min={1}
            max={100}
            value={summarizeThreshold}
            onChange={(e) => {
              const value = parseInt(e.target.value, 10);
              if (!isNaN(value)) {
                setPendingChange('memory', 'summarize_threshold', value);
              }
            }}
          />
          <p className="text-xs text-muted-foreground">
            Number of messages before triggering summarization
          </p>
        </div>

        <div className="space-y-2">
          <Label htmlFor="max-stored-messages">Max Stored Messages</Label>
          <Input
            id="max-stored-messages"
            type="number"
            min={10}
            max={1000}
            value={maxStoredMessages}
            onChange={(e) => {
              const value = parseInt(e.target.value, 10);
              if (!isNaN(value)) {
                setPendingChange('memory', 'max_stored_messages', value);
              }
            }}
          />
          <p className="text-xs text-muted-foreground">
            Maximum messages to store per conversation
          </p>
        </div>
      </div>

      {/* RAG Settings */}
      <div className="space-y-4">
        <h4 className="font-medium">RAG Settings</h4>

        <div className="flex items-center justify-between">
          <div className="space-y-0.5">
            <Label>Enable RAG</Label>
            <p className="text-xs text-muted-foreground">
              Enable Retrieval-Augmented Generation for context enhancement
            </p>
          </div>
          <Switch
            checked={ragEnable}
            onCheckedChange={(checked) => setPendingChange('rag', 'enable', checked)}
          />
        </div>
      </div>
    </div>
  );
}
