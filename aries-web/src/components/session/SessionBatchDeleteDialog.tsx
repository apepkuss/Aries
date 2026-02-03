import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';

interface SessionBatchDeleteDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onConfirm: () => void;
  /** Number of selected sessions. 0 means "delete all". */
  count: number;
}

export function SessionBatchDeleteDialog({
  open,
  onOpenChange,
  onConfirm,
  count,
}: SessionBatchDeleteDialogProps) {
  const isDeleteAll = count === 0;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-sm">
        <DialogHeader>
          <DialogTitle>
            {isDeleteAll ? '清空全部会话' : `删除 ${count} 个会话`}
          </DialogTitle>
          <DialogDescription>
            {isDeleteAll
              ? '确定要清空所有会话记录吗？此操作不可撤销。'
              : `确定要删除选中的 ${count} 个会话吗？此操作不可撤销。`}
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            取消
          </Button>
          <Button
            variant="destructive"
            onClick={() => {
              onConfirm();
              onOpenChange(false);
            }}
          >
            {isDeleteAll ? '清空全部' : '删除'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
