/**
 * Announcements API stub for allo canvas.
 */

export type AnnouncementLevel = 'info' | 'success' | 'warning' | 'critical';
export type AnnouncementStatus = 'active' | 'closed';

export type SystemAnnouncement = {
  id: string;
  title: string;
  content: string;
  level: AnnouncementLevel;
  status: AnnouncementStatus;
  createdBy: string;
  publishedAt: string;
  closedAt?: string;
  createdAt: string;
  updatedAt: string;
};

export type AnnouncementFeed = {
  announcements: SystemAnnouncement[];
  unreadCount: number;
};

export type AdminAnnouncementListParams = {
  keyword?: string;
  status?: AnnouncementStatus;
  page?: number;
  limit?: number;
};

export function getAnnouncementFeed() {
  return Promise.resolve({ announcements: [], unreadCount: 0 } as AnnouncementFeed);
}

export function markAnnouncementsRead(_announcementIds: string[]) {
  return Promise.resolve({ unreadCount: 0 });
}

export function listAdminAnnouncements(_params: AdminAnnouncementListParams = {}) {
  return Promise.resolve({ announcements: [] as SystemAnnouncement[], total: 0, page: 1, limit: 30 });
}

export function createAdminAnnouncement(_input: {
  title: string;
  content: string;
  level: AnnouncementLevel;
}) {
  return Promise.reject(new Error('Admin announcements are not available in allo canvas'));
}

export function updateAdminAnnouncement(
  _id: string,
  _input: { title: string; content: string; level: AnnouncementLevel }
) {
  return Promise.reject(new Error('Admin announcements are not available in allo canvas'));
}

export function closeAdminAnnouncement(_id: string) {
  return Promise.reject(new Error('Admin announcements are not available in allo canvas'));
}
