import { useEffect, useCallback } from 'react';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { useAriesStore } from '../store';
import type { StreamEvent } from '../types/execution';
import { getEventType } from '../types/execution';

/**
 * Hook to subscribe to Plan mode execution events.
 *
 * This hook sets up a Tauri event listener for the given request ID
 * and automatically updates the store with received events.
 */
export function useExecutionEvents() {
  const {
    execution,
    addExecutionStep,
    completeExecution,
    failExecution,
    resetExecution,
  } = useAriesStore();

  const { requestId, isExecuting } = execution;

  // Handle incoming events
  const handleEvent = useCallback(
    (event: StreamEvent) => {
      const eventType = getEventType(event);

      // Add the step to execution state
      addExecutionStep(event);

      // Handle specific event types
      if (eventType === 'text') {
        const textEvent = event as { type: 'text'; content: string };
        useAriesStore.getState().appendStreamingText(textEvent.content);
      } else if (eventType === 'finish') {
        const finishEvent = event as { type: 'finish' } & import('../types/execution').FinishEvent;
        completeExecution({
          content: '', // Actual content was accumulated via text events
          usage: finishEvent.usage,
        });
      }
    },
    [addExecutionStep, completeExecution, failExecution]
  );

  // Set up event listener
  useEffect(() => {
    if (!requestId || !isExecuting) {
      return;
    }

    let unlisten: UnlistenFn | null = null;

    const setupListener = async () => {
      const eventName = `plan-event-${requestId}`;
      console.log(`[ExecutionEvents] Setting up listener for: ${eventName}`);

      try {
        unlisten = await listen<StreamEvent>(eventName, (event) => {
          console.log(`[ExecutionEvents] Received event:`, event.payload);
          handleEvent(event.payload);
        });
        console.log(`[ExecutionEvents] Listener setup complete`);
      } catch (error) {
        console.error(`[ExecutionEvents] Failed to setup listener:`, error);
      }
    };

    setupListener();

    // Cleanup listener on unmount or when requestId changes
    return () => {
      if (unlisten) {
        console.log(`[ExecutionEvents] Cleaning up listener for: plan-event-${requestId}`);
        unlisten();
      }
    };
  }, [requestId, isExecuting, handleEvent]);

  // Return execution state for convenience
  return {
    isExecuting,
    requestId,
    phase: execution.phase,
    steps: execution.steps,
    statusMessage: execution.statusMessage,
    subtaskProgress: execution.subtaskProgress,
    result: execution.result,
    error: execution.error,
    resetExecution,
  };
}

/**
 * Hook to send a message with Plan mode execution transparency.
 *
 * Returns a function to send messages and the execution state.
 */
export function usePlanChat() {
  const { sendMessageWithPlan, isLoading } = useAriesStore();
  const executionState = useExecutionEvents();

  const sendMessage = useCallback(
    async (content: string) => {
      const requestId = await sendMessageWithPlan(content);
      return requestId;
    },
    [sendMessageWithPlan]
  );

  return {
    sendMessage,
    isLoading,
    ...executionState,
  };
}
