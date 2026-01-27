import { cn } from '@/lib/utils';
import type {
  HitlOperationPreview,
  FileOperationPreview,
  ShellCommandPreview,
  HttpRequestPreview,
  GenericPreview,
} from '@/api/types';
import {
  FileIcon,
  FilePlusIcon,
  FileEditIcon,
  FileMinusIcon,
  FolderInputIcon,
  CopyIcon,
  TerminalIcon,
  GlobeIcon,
  InfoIcon,
} from 'lucide-react';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';

interface OperationPreviewProps {
  preview: HitlOperationPreview;
  className?: string;
}

export function OperationPreview({ preview, className }: OperationPreviewProps) {
  switch (preview.type) {
    case 'file':
      return <FilePreview preview={preview} className={className} />;
    case 'shell':
      return <ShellPreview preview={preview} className={className} />;
    case 'http':
      return <HttpPreview preview={preview} className={className} />;
    case 'generic':
      return <GenericPreviewComponent preview={preview} className={className} />;
    default:
      return null;
  }
}

// File operation preview
function FilePreview({
  preview,
  className,
}: {
  preview: FileOperationPreview;
  className?: string;
}) {
  const operationIcons = {
    create: FilePlusIcon,
    modify: FileEditIcon,
    delete: FileMinusIcon,
    move: FolderInputIcon,
    copy: CopyIcon,
  };

  const operationLabels = {
    create: 'Create File',
    modify: 'Modify File',
    delete: 'Delete File',
    move: 'Move File',
    copy: 'Copy File',
  };

  const operationColors = {
    create: 'text-green-600 dark:text-green-400',
    modify: 'text-yellow-600 dark:text-yellow-400',
    delete: 'text-red-600 dark:text-red-400',
    move: 'text-blue-600 dark:text-blue-400',
    copy: 'text-purple-600 dark:text-purple-400',
  };

  const Icon = operationIcons[preview.operation] || FileIcon;
  const label = operationLabels[preview.operation] || preview.operation;
  const color = operationColors[preview.operation] || 'text-gray-600';

  return (
    <Card className={cn('overflow-hidden', className)}>
      <CardHeader className="pb-2">
        <CardTitle className="flex items-center gap-2 text-sm">
          <Icon className={cn('h-4 w-4', color)} />
          <span>{label}</span>
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-2">
        <div className="flex items-center gap-2 text-sm">
          <span className="text-muted-foreground">Path:</span>
          <code className="rounded bg-muted px-1.5 py-0.5 text-xs font-mono">
            {preview.path}
          </code>
        </div>

        {preview.size_bytes !== undefined && (
          <div className="flex items-center gap-2 text-sm">
            <span className="text-muted-foreground">Size:</span>
            <span>{formatBytes(preview.size_bytes)}</span>
          </div>
        )}

        {preview.is_binary && (
          <div className="text-xs text-muted-foreground">(Binary file)</div>
        )}

        {preview.content_preview && (
          <div className="mt-2">
            <div className="text-xs text-muted-foreground mb-1">Content Preview:</div>
            <pre className="rounded bg-muted p-2 text-xs font-mono overflow-x-auto max-h-40">
              {preview.content_preview}
            </pre>
          </div>
        )}

        {preview.original_content && preview.operation === 'modify' && (
          <div className="mt-2">
            <div className="text-xs text-muted-foreground mb-1">Original Content:</div>
            <pre className="rounded bg-muted p-2 text-xs font-mono overflow-x-auto max-h-40 opacity-60">
              {preview.original_content}
            </pre>
          </div>
        )}
      </CardContent>
    </Card>
  );
}

// Shell command preview
function ShellPreview({
  preview,
  className,
}: {
  preview: ShellCommandPreview;
  className?: string;
}) {
  return (
    <Card className={cn('overflow-hidden', className)}>
      <CardHeader className="pb-2">
        <CardTitle className="flex items-center gap-2 text-sm">
          <TerminalIcon className="h-4 w-4 text-green-600 dark:text-green-400" />
          <span>Shell Command</span>
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-2">
        <div>
          <div className="text-xs text-muted-foreground mb-1">Command:</div>
          <pre className="rounded bg-black text-green-400 p-2 text-xs font-mono overflow-x-auto">
            $ {preview.command}
          </pre>
        </div>

        {preview.working_directory && (
          <div className="flex items-center gap-2 text-sm">
            <span className="text-muted-foreground">Working Directory:</span>
            <code className="rounded bg-muted px-1.5 py-0.5 text-xs font-mono">
              {preview.working_directory}
            </code>
          </div>
        )}

        {preview.environment && Object.keys(preview.environment).length > 0 && (
          <div>
            <div className="text-xs text-muted-foreground mb-1">Environment:</div>
            <div className="space-y-1">
              {Object.entries(preview.environment).map(([key, value]) => (
                <div key={key} className="text-xs">
                  <code className="text-blue-600 dark:text-blue-400">{key}</code>
                  <span className="text-muted-foreground">=</span>
                  <code className="text-green-600 dark:text-green-400">{value}</code>
                </div>
              ))}
            </div>
          </div>
        )}

        {preview.estimated_impact && preview.estimated_impact.length > 0 && (
          <div>
            <div className="text-xs text-muted-foreground mb-1">Estimated Impact:</div>
            <div className="flex flex-wrap gap-1">
              {preview.estimated_impact.map((impact, i) => (
                <span
                  key={i}
                  className="rounded-full bg-yellow-100 dark:bg-yellow-900/30 px-2 py-0.5 text-xs text-yellow-700 dark:text-yellow-400"
                >
                  {impact}
                </span>
              ))}
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  );
}

// HTTP request preview
function HttpPreview({
  preview,
  className,
}: {
  preview: HttpRequestPreview;
  className?: string;
}) {
  const methodColors: Record<string, string> = {
    GET: 'text-green-600 dark:text-green-400',
    POST: 'text-blue-600 dark:text-blue-400',
    PUT: 'text-yellow-600 dark:text-yellow-400',
    PATCH: 'text-orange-600 dark:text-orange-400',
    DELETE: 'text-red-600 dark:text-red-400',
  };

  return (
    <Card className={cn('overflow-hidden', className)}>
      <CardHeader className="pb-2">
        <CardTitle className="flex items-center gap-2 text-sm">
          <GlobeIcon className="h-4 w-4 text-blue-600 dark:text-blue-400" />
          <span>HTTP Request</span>
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-2">
        <div className="flex items-center gap-2">
          <span
            className={cn(
              'font-mono font-bold',
              methodColors[preview.method.toUpperCase()] || 'text-gray-600'
            )}
          >
            {preview.method.toUpperCase()}
          </span>
          <code className="rounded bg-muted px-1.5 py-0.5 text-xs font-mono truncate flex-1">
            {preview.url}
          </code>
        </div>

        {preview.headers && Object.keys(preview.headers).length > 0 && (
          <div>
            <div className="text-xs text-muted-foreground mb-1">Headers:</div>
            <div className="space-y-1">
              {Object.entries(preview.headers).map(([key, value]) => (
                <div key={key} className="text-xs font-mono">
                  <span className="text-muted-foreground">{key}:</span> {value}
                </div>
              ))}
            </div>
          </div>
        )}

        {preview.body_preview && (
          <div>
            <div className="text-xs text-muted-foreground mb-1">Body:</div>
            <pre className="rounded bg-muted p-2 text-xs font-mono overflow-x-auto max-h-32">
              {preview.body_preview}
            </pre>
          </div>
        )}
      </CardContent>
    </Card>
  );
}

// Generic preview
function GenericPreviewComponent({
  preview,
  className,
}: {
  preview: GenericPreview;
  className?: string;
}) {
  return (
    <Card className={cn('overflow-hidden', className)}>
      <CardHeader className="pb-2">
        <CardTitle className="flex items-center gap-2 text-sm">
          <InfoIcon className="h-4 w-4" />
          <span>{preview.title}</span>
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-2">
        <p className="text-sm text-muted-foreground">{preview.description}</p>

        {preview.details && Object.keys(preview.details).length > 0 && (
          <div className="space-y-1">
            {Object.entries(preview.details).map(([key, value]) => (
              <div key={key} className="flex items-start gap-2 text-sm">
                <span className="text-muted-foreground min-w-20">{key}:</span>
                <span className="break-all">{value}</span>
              </div>
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  );
}

// Helper function to format bytes
function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 Bytes';
  const k = 1024;
  const sizes = ['Bytes', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
}
