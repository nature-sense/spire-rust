import type { MethodHandler } from '../router';
import type { ChatDialog, ChatMessage } from '../../model/types';

/**
 * Callback type for chat event notifications.
 * Set by the extension to wire up event emission.
 */
export let notifyChatEvent: ((method: string, params: unknown) => void) | null = null;

/**
 * Register the notification callback (called by extension.ts during setup).
 */
export function setChatNotifier(notifier: (method: string, params: unknown) => void): void {
  notifyChatEvent = notifier;
}

/**
 * In-memory chat store.
 * In a real implementation this would integrate with the VS Code Chat API
 * or the Spire chat service.
 */
const chats = new Map<string, ChatDialog>();

/** Get or create a default chat */
function getDefaultChat(): ChatDialog {
  for (const chat of chats.values()) {
    return chat;
  }
  const now = new Date().toISOString();
  const chat: ChatDialog = {
    id: 'default',
    title: 'New Chat',
    messages: [],
    status: 'idle',
    createdAt: now,
    updatedAt: now,
  };
  chats.set(chat.id, chat);
  return chat;
}

export const chatHandlers: Record<string, MethodHandler> = {
  'chat/getActive': async () => {
    return getDefaultChat();
  },

  'chat/getHistory': async () => {
    return Array.from(chats.values());
  },

  'chat/getMessage': async (params: unknown) => {
    const { chatId, messageId } = params as { chatId: string; messageId: string };
    const chat = chats.get(chatId);
    if (!chat) return null;
    return chat.messages.find(m => m.id === messageId) ?? null;
  },

  'chat/append': async (params: unknown) => {
    const { chatId, content, options } = params as {
      chatId: string;
      content: string;
      options?: { role?: ChatMessage['role']; metadata?: Record<string, unknown> };
    };
    let chat = chats.get(chatId);
    if (!chat) {
      // Auto-create chat
      const now = new Date().toISOString();
      chat = {
        id: chatId,
        title: 'New Chat',
        messages: [],
        status: 'idle',
        createdAt: now,
        updatedAt: now,
      };
      chats.set(chatId, chat);
    }

    const message: ChatMessage = {
      id: `msg-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
      role: options?.role ?? 'assistant',
      content,
      timestamp: new Date().toISOString(),
      metadata: options?.metadata,
    };

    chat.messages.push(message);
    chat.updatedAt = message.timestamp;

    // Emit event notification for new messages
    if (notifyChatEvent) {
      notifyChatEvent('event/chat/message', {
        chatId,
        message,
      });
    }

    return message;
  },

  'chat/clear': async (params: unknown) => {
    const { chatId } = params as { chatId: string };
    const chat = chats.get(chatId);
    if (chat) {
      chat.messages = [];
      chat.updatedAt = new Date().toISOString();
    }
  },

  'chat/setTitle': async (params: unknown) => {
    const { chatId, title } = params as { chatId: string; title: string };
    const chat = chats.get(chatId);
    if (chat) {
      chat.title = title;
      chat.updatedAt = new Date().toISOString();
    }
  },

  'chat/show': async (_params: unknown) => {
    // In a real implementation, this would call vscode.commands.executeCommand
    // to focus the chat panel
  },
};
