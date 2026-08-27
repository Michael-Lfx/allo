/**
 * Warm the meeting page route before the sider click.
 */
export function prefetchMeetingPage(): void {
  void import('./MeetingPage');
  void import('./MeetingDetailPage');
}
