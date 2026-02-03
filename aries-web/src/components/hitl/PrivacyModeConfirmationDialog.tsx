import { useState, useEffect, useCallback } from 'react';
import { cn } from '@/lib/utils';
import type { UIHitlRequest, HitlPrivacyModeConfirmationRequest } from '@/api/types';
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Switch } from '@/components/ui/switch';
import { Label } from '@/components/ui/label';
import {
  ShieldIcon,
  ShieldAlertIcon,
  MessageSquareIcon,
  Loader2Icon,
} from 'lucide-react';

interface PrivacyModeConfirmationDialogProps {
  request: UIHitlRequest | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onChoosePrivacyMode: (requestId: string, usePrivacyMode: boolean, rememberChoice: boolean) => Promise<void>;
  className?: string;
}

export function PrivacyModeConfirmationDialog({
  request,
  open,
  onOpenChange,
  onChoosePrivacyMode,
  className,
}: PrivacyModeConfirmationDialogProps) {
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [rememberChoice, setRememberChoice] = useState(false);

  // Reset state when dialog opens/closes or request changes
  useEffect(() => {
    if (open) {
      setIsSubmitting(false);
      setRememberChoice(false);
    }
  }, [open, request?.id]);

  const handleChoosePrivacyMode = useCallback(async (usePrivacyMode: boolean) => {
    if (!request) return;
    setIsSubmitting(true);
    try {
      await onChoosePrivacyMode(request.id, usePrivacyMode, rememberChoice);
      onOpenChange(false);
    } finally {
      setIsSubmitting(false);
    }
  }, [request, onChoosePrivacyMode, onOpenChange, rememberChoice]);

  if (!request) return null;

  const privacyData =
    request.request_type.type === 'privacy_mode_confirmation'
      ? (request.request_type.data as HitlPrivacyModeConfirmationRequest)
      : null;

  if (!privacyData) return null;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className={cn('max-w-sm', className)}
        showCloseButton={!isSubmitting}
      >
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <ShieldAlertIcon className="h-5 w-5 text-amber-500" />
            隐私保护
          </DialogTitle>
        </DialogHeader>

        <div className="space-y-4 py-2">
          <p className="text-sm text-muted-foreground">
            您的查询中可能包含敏感信息，建议使用隐私模式，以保护您的隐私。
          </p>

          {/* Remember Choice Switch */}
          <div className="flex items-center space-x-2">
            <Switch
              id="remember-choice"
              checked={rememberChoice}
              onCheckedChange={setRememberChoice}
              disabled={isSubmitting}
            />
            <Label
              htmlFor="remember-choice"
              className="text-sm text-muted-foreground cursor-pointer"
            >
              本次会话记住我的选择
            </Label>
          </div>
        </div>

        <DialogFooter className="flex-col gap-2 sm:flex-row">
          <Button
            variant="default"
            onClick={() => handleChoosePrivacyMode(true)}
            disabled={isSubmitting}
            className="gap-2 bg-emerald-600 hover:bg-emerald-700 text-white"
          >
            {isSubmitting ? (
              <Loader2Icon className="h-4 w-4 animate-spin" />
            ) : (
              <ShieldIcon className="h-4 w-4" />
            )}
            使用隐私模式
          </Button>

          <Button
            variant="outline"
            onClick={() => handleChoosePrivacyMode(false)}
            disabled={isSubmitting}
            className="gap-2"
          >
            <MessageSquareIcon className="h-4 w-4" />
            继续使用普通模式
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
