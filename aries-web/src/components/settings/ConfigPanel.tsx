import { useEffect } from 'react';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Button } from '@/components/ui/button';
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
import { useUIStore, useConfigStore, useServiceStore } from '@/stores';
import { ServiceConfigForm } from './ServiceConfigForm';
import { ServerParamsForm } from './ServerParamsForm';
import { MemoryConfigForm } from './MemoryConfigForm';

export function ConfigPanel() {
  const { settingsOpen, setSettingsOpen } = useUIStore();
  const {
    isLoading,
    isSaving,
    error,
    fetchConfig,
    fetchSchema,
    saveChanges,
    clearPendingChanges,
    hasPendingChanges,
  } = useConfigStore();

  const {
    hasChatUrlChange,
    hasPrivacyChatUrlChange,
    hasServicePendingChanges,
    chatTestState,
    privacyChatTestState,
    connectChat,
    connectPrivacyChat,
    clearPending: clearServicePending,
  } = useServiceStore();

  // Fetch config when dialog opens
  useEffect(() => {
    if (settingsOpen) {
      fetchConfig();
      fetchSchema();
    }
  }, [settingsOpen, fetchConfig, fetchSchema]);

  // Determine button state for service tab
  const chatNeedsConnect = hasChatUrlChange() && chatTestState !== 'passed';
  const privacyNeedsConnect = hasPrivacyChatUrlChange() && privacyChatTestState !== 'passed';
  const needsConnect = chatNeedsConnect || privacyNeedsConnect;
  const isConnecting = chatTestState === 'testing' || privacyChatTestState === 'testing';

  // Handle connect (register service via admin API)
  const handleConnect = async () => {
    let allSuccess = true;

    if (chatNeedsConnect) {
      const success = await connectChat();
      if (success) {
        toast.success('Chat service connected successfully');
      } else {
        toast.error('Chat service connection failed');
        allSuccess = false;
      }
    }

    if (privacyNeedsConnect) {
      const success = await connectPrivacyChat();
      if (success) {
        toast.success('Privacy chat service connected successfully');
      } else {
        toast.error('Privacy chat service connection failed');
        allSuccess = false;
      }
    }

    return allSuccess;
  };

  // Handle save (for params/memory changes via config API)
  const handleSave = async () => {
    const response = await saveChanges();
    if (response) {
      if (response.success) {
        toast.success('Configuration saved successfully');

        if (response.requires_action && Object.keys(response.requires_action).length > 0) {
          Object.entries(response.requires_action).forEach(([field, action]) => {
            toast.info(`${field}: ${action}`);
          });
        }
      } else {
        toast.error(response.message);

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
    clearServicePending();
    setSettingsOpen(false);
  };

  // Determine which button to show
  const hasConfigChanges = hasPendingChanges();
  const hasAnyServiceChanges = hasServicePendingChanges();

  return (
    <Dialog open={settingsOpen} onOpenChange={(open) => !open && handleClose()}>
      <DialogContent className="max-w-2xl max-h-[85vh] flex flex-col">
        <DialogHeader className="shrink-0">
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
            <ScrollArea className="flex-1 min-h-0 overflow-hidden">
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

            <DialogFooter className="shrink-0 gap-2 border-t pt-4">
              <div className="flex gap-2 w-full sm:w-auto justify-end">
                <Button variant="outline" onClick={handleClose}>
                  Cancel
                </Button>
                {needsConnect ? (
                  <Button onClick={handleConnect} disabled={isConnecting}>
                    {isConnecting && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
                    Connect
                  </Button>
                ) : (hasConfigChanges || hasAnyServiceChanges) ? (
                  <Button
                    onClick={async () => {
                      if (hasAnyServiceChanges) {
                        await handleConnect();
                      }
                      if (hasConfigChanges) {
                        await handleSave();
                      }
                    }}
                    disabled={isSaving}
                  >
                    {isSaving && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
                    Save
                  </Button>
                ) : (
                  <Button disabled>Save</Button>
                )}
              </div>
            </DialogFooter>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
