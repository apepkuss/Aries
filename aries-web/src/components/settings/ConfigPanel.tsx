import { useEffect } from 'react';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Loader2, AlertTriangle } from 'lucide-react';
import { toast } from 'sonner';
import { useUIStore, useConfigStore } from '@/stores';
import { ServiceConfigForm } from './ServiceConfigForm';
import { ServerParamsForm } from './ServerParamsForm';
import { MemoryConfigForm } from './MemoryConfigForm';

export function ConfigPanel() {
  const { settingsOpen, setSettingsOpen } = useUIStore();
  const {
    isLoading,
    isSaving,
    error,
    pendingChanges,
    fetchConfig,
    fetchSchema,
    saveChanges,
    clearPendingChanges,
    hasPendingChanges,
  } = useConfigStore();

  // Fetch config when dialog opens
  useEffect(() => {
    if (settingsOpen) {
      fetchConfig();
      fetchSchema();
    }
  }, [settingsOpen, fetchConfig, fetchSchema]);

  // Handle save
  const handleSave = async () => {
    const response = await saveChanges();
    if (response) {
      if (response.success) {
        toast.success('Configuration saved successfully');

        // Show side effect warnings
        if (response.requires_action && Object.keys(response.requires_action).length > 0) {
          Object.entries(response.requires_action).forEach(([field, action]) => {
            toast.info(`${field}: ${action}`);
          });
        }
      } else {
        toast.error(response.message);

        // Show failed fields
        if (response.failed_fields) {
          Object.entries(response.failed_fields).forEach(([field, error]) => {
            toast.error(`${field}: ${error}`);
          });
        }
      }
    }
  };

  // Handle close
  const handleClose = () => {
    clearPendingChanges();
    setSettingsOpen(false);
  };

  // Check if there are changes to chat service that will cause reconnection
  const hasServiceChanges = !!(
    pendingChanges.chat?.url ||
    pendingChanges.chat?.api_key
  );

  return (
    <Dialog open={settingsOpen} onOpenChange={(open) => !open && handleClose()}>
      <DialogContent className="max-w-2xl max-h-[85vh]">
        <DialogHeader>
          <DialogTitle>Settings</DialogTitle>
          <DialogDescription>
            Configure Aries server settings. Changes are applied immediately after saving.
          </DialogDescription>
        </DialogHeader>

        {isLoading ? (
          <div className="flex items-center justify-center py-12">
            <Loader2 className="h-8 w-8 animate-spin text-muted-foreground" />
          </div>
        ) : error ? (
          <div className="flex flex-col items-center justify-center py-12 gap-2">
            <AlertTriangle className="h-8 w-8 text-destructive" />
            <p className="text-sm text-destructive">{error}</p>
            <Button variant="outline" onClick={() => fetchConfig()}>
              Retry
            </Button>
          </div>
        ) : (
          <>
            <ScrollArea className="max-h-[60vh]">
              <Tabs defaultValue="service" className="w-full">
                <TabsList className="w-full">
                  <TabsTrigger value="service" className="flex-1">
                    Service
                  </TabsTrigger>
                  <TabsTrigger value="params" className="flex-1">
                    Parameters
                  </TabsTrigger>
                  <TabsTrigger value="memory" className="flex-1">
                    Memory
                  </TabsTrigger>
                </TabsList>

                <div className="mt-4 px-1">
                  <TabsContent value="service">
                    <ServiceConfigForm />
                  </TabsContent>

                  <TabsContent value="params">
                    <ServerParamsForm />
                  </TabsContent>

                  <TabsContent value="memory">
                    <MemoryConfigForm />
                  </TabsContent>
                </div>
              </Tabs>
            </ScrollArea>

            {/* Warning for service changes */}
            {hasServiceChanges && (
              <div className="flex items-center gap-2 p-3 bg-yellow-500/10 border border-yellow-500/20 rounded-md">
                <AlertTriangle className="h-4 w-4 text-yellow-500" />
                <p className="text-sm text-yellow-600 dark:text-yellow-400">
                  Changing service URL or API key will reconnect the service.
                </p>
              </div>
            )}

            <DialogFooter className="gap-2 sm:gap-0">
              {hasPendingChanges() && (
                <Badge variant="secondary" className="mr-auto">
                  Unsaved changes
                </Badge>
              )}
              <Button variant="outline" onClick={handleClose}>
                Cancel
              </Button>
              <Button onClick={handleSave} disabled={!hasPendingChanges() || isSaving}>
                {isSaving && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
                Save Changes
              </Button>
            </DialogFooter>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
