import { cn } from '@/lib/utils';
import type { HitlRiskLevel } from '@/api/types';
import {
  ShieldAlertIcon,
  ShieldCheckIcon,
  AlertTriangleIcon,
  AlertOctagonIcon,
} from 'lucide-react';

interface RiskBadgeProps {
  level: HitlRiskLevel;
  showIcon?: boolean;
  showLabel?: boolean;
  size?: 'sm' | 'md' | 'lg';
  className?: string;
}

const riskConfig: Record<
  HitlRiskLevel,
  {
    label: string;
    bgColor: string;
    textColor: string;
    borderColor: string;
    icon: typeof ShieldCheckIcon;
  }
> = {
  safe: {
    label: 'Safe',
    bgColor: 'bg-blue-100 dark:bg-blue-900/30',
    textColor: 'text-blue-700 dark:text-blue-400',
    borderColor: 'border-blue-300 dark:border-blue-700',
    icon: ShieldCheckIcon,
  },
  low: {
    label: 'Low Risk',
    bgColor: 'bg-green-100 dark:bg-green-900/30',
    textColor: 'text-green-700 dark:text-green-400',
    borderColor: 'border-green-300 dark:border-green-700',
    icon: ShieldCheckIcon,
  },
  medium: {
    label: 'Medium Risk',
    bgColor: 'bg-yellow-100 dark:bg-yellow-900/30',
    textColor: 'text-yellow-700 dark:text-yellow-400',
    borderColor: 'border-yellow-300 dark:border-yellow-700',
    icon: AlertTriangleIcon,
  },
  high: {
    label: 'High Risk',
    bgColor: 'bg-orange-100 dark:bg-orange-900/30',
    textColor: 'text-orange-700 dark:text-orange-400',
    borderColor: 'border-orange-300 dark:border-orange-700',
    icon: ShieldAlertIcon,
  },
  critical: {
    label: 'Critical Risk',
    bgColor: 'bg-red-100 dark:bg-red-900/30',
    textColor: 'text-red-700 dark:text-red-400',
    borderColor: 'border-red-300 dark:border-red-700',
    icon: AlertOctagonIcon,
  },
};

const sizeClasses = {
  sm: 'px-1.5 py-0.5 text-xs gap-1',
  md: 'px-2 py-1 text-sm gap-1.5',
  lg: 'px-3 py-1.5 text-base gap-2',
};

const iconSizes = {
  sm: 'h-3 w-3',
  md: 'h-4 w-4',
  lg: 'h-5 w-5',
};

export function RiskBadge({
  level,
  showIcon = true,
  showLabel = true,
  size = 'md',
  className,
}: RiskBadgeProps) {
  // Fallback to medium if level is not recognized
  const config = riskConfig[level] || riskConfig.medium;
  const Icon = config.icon;

  return (
    <span
      className={cn(
        'inline-flex items-center rounded-full border font-medium',
        config.bgColor,
        config.textColor,
        config.borderColor,
        sizeClasses[size],
        className
      )}
    >
      {showIcon && <Icon className={iconSizes[size]} />}
      {showLabel && <span>{config.label}</span>}
    </span>
  );
}

export function RiskLevelIcon({
  level,
  className,
}: {
  level: HitlRiskLevel;
  className?: string;
}) {
  // Fallback to medium if level is not recognized
  const config = riskConfig[level] || riskConfig.medium;
  const Icon = config.icon;
  return <Icon className={cn(config.textColor, className)} />;
}
