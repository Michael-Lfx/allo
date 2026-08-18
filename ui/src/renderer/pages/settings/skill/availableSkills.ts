import { ipcBridge } from '@/common';

export const AVAILABLE_SKILLS_SWR_KEY = 'skills.available';

export const fetchAvailableSkills = () => ipcBridge.fs.listAvailableSkills.invoke();
