import { cn } from '@/lib/utils';
import type { HitlRequestStatus } from '@/api/types';
import {
  CheckIcon,
  XIcon,
  PencilIcon,
  AlertCircleIcon,
  BanIcon,
  CheckCircleIcon,
  Loader2Icon,
} from 'lucide-react';

interface HitlStatusIndicatorProps {
  status: HitlRequestStatus;
  size?: 'sm' | 'md' | 'lg';
  showLabel?: boolean;
  className?: string;
}

const statusConfig: Record<
  HitlRequestStatus,
  {
    icon: React.ComponentType<{ className?: string }>;
    label: string;
    color: string;
    bgColor: string;
  }
> = {
  pending: {
    icon: Loader2Icon,
    label: 'Pending',
    color: 'text-yellow-600 dark:text-yellow-400',
    bgColor: 'bg-yellow-100 dark:bg-yellow-900/30',
  },
  approved: {
    icon: CheckIcon,
    label: 'Approved',
    color: 'text-green-600 dark:text-green-400',
    bgColor: 'bg-green-100 dark:bg-green-900/30',
  },
  rejected: {
    icon: XIcon,
    label: 'Rejected',
    color: 'text-red-600 dark:text-red-400',
    bgColor: 'bg-red-100 dark:bg-red-900/30',
  },
  modified: {
    icon: PencilIcon,
    label: 'Modified',
    color: 'text-purple-600 dark:text-purple-400',
    bgColor: 'bg-purple-100 dark:bg-purple-900/30',
  },
  expired: {
    icon: AlertCircleIcon,
    label: 'Expired',
    color: 'text-gray-600 dark:text-gray-400',
    bgColor: 'bg-gray-100 dark:bg-gray-900/30',
  },
  cancelled: {
    icon: BanIcon,
    label: 'Cancelled',
    color: 'text-gray-600 dark:text-gray-400',
    bgColor: 'bg-gray-100 dark:bg-gray-900/30',
  },
  completed: {
    icon: CheckCircleIcon,
    label: 'Completed',
    color: 'text-green-600 dark:text-green-400',
    bgColor: 'bg-green-100 dark:bg-green-900/30',
  },
};

const sizeConfig = {
  sm: {
    container: 'px-1.5 py-0.5 text-xs',
    icon: 'h-3 w-3',
    gap: 'gap-1',
  },
  md: {
    container: 'px-2 py-1 text-sm',
    icon: 'h-4 w-4',
    gap: 'gap-1.5',
  },
  lg: {
    container: 'px-3 py-1.5 text-base',
    icon: 'h-5 w-5',
    gap: 'gap-2',
  },
};

export function HitlStatusIndicator({
  status,
  size = 'md',
  showLabel = true,
  className,
}: HitlStatusIndicatorProps) {
  const config = statusConfig[status];
  const sizeStyles = sizeConfig[size];

  if (!config) {
    return null;
  }

  const Icon = config.icon;
  const isAnimated = status === 'pending';

  return (
    <span
      className={cn(
        'inline-flex items-center rounded-full font-medium',
        sizeStyles.container,
        sizeStyles.gap,
        config.bgColor,
        config.color,
        className
      )}
    >
      <Icon className={cn(sizeStyles.icon, isAnimated && 'animate-spin')} />
      {showLabel && <span>{config.label}</span>}
    </span>
  );
}

// Simple dot indicator for compact display
interface HitlStatusDotProps {
  status: HitlRequestStatus;
  size?: 'sm' | 'md' | 'lg';
  className?: string;
}

const dotSizes = {
  sm: 'h-2 w-2',
  md: 'h-2.5 w-2.5',
  lg: 'h-3 w-3',
};

const dotColors: Record<HitlRequestStatus, string> = {
  pending: 'bg-yellow-500 animate-pulse',
  approved: 'bg-green-500',
  rejected: 'bg-red-500',
  modified: 'bg-purple-500',
  expired: 'bg-gray-500',
  cancelled: 'bg-gray-500',
  completed: 'bg-green-500',
};

export function HitlStatusDot({ status, size = 'md', className }: HitlStatusDotProps) {
  return (
    <span
      className={cn('inline-block rounded-full', dotSizes[size], dotColors[status], className)}
      title={statusConfig[status]?.label || status}
    />
  );
}
