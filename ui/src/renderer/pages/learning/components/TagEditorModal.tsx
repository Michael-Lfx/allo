import { Checkbox, Input, Modal, Select } from '@arco-design/web-react';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';

/** 课程 / 题目共用的标签编辑弹窗：从已有标签中选择或输入新标签 */
export function TagEditorModal({
  title,
  initialTags,
  allTags,
  busy,
  showApplyToChildren,
  onConfirm,
  onClose,
}: {
  title: string;
  initialTags: string[];
  allTags: string[];
  busy: boolean;
  showApplyToChildren?: boolean;
  onConfirm: (tags: string[], applyToChildren: boolean) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [tags, setTags] = useState<string[]>(initialTags);
  const [newTag, setNewTag] = useState('');
  const [applyToChildren, setApplyToChildren] = useState(true);
  useEffect(() => {
    setTags(initialTags);
    setNewTag('');
    setApplyToChildren(true);
  }, [initialTags]);
  const addNewTag = () => {
    const name = newTag.trim();
    if (name === '') return;
    setTags((current) => (current.includes(name) ? current : [...current, name]));
    setNewTag('');
  };
  const known = allTags.filter((tag) => !tags.includes(tag));
  return (
    <Modal
      title={title}
      visible
      style={{ width: 480 }}
      confirmLoading={busy}
      okText={t('learning.tagsSave')}
      onCancel={() => {
        if (!busy) onClose();
      }}
      onOk={() => onConfirm(tags, applyToChildren)}
    >
      <div className='flex flex-col gap-14px'>
        <div>
          <div className='mb-6px text-13px'>{t('learning.tagsLabel')}</div>
          <Select
            mode='multiple'
            className='w-full'
            value={tags}
            placeholder={t('learning.tagsPlaceholder')}
            onChange={(value: string[]) => setTags(value)}
          >
            {known.map((tag) => (
              <Select.Option key={tag} value={tag}>
                {tag}
              </Select.Option>
            ))}
          </Select>
        </div>
        <div>
          <div className='mb-6px text-13px'>{t('learning.tagsNewLabel')}</div>
          <Input.Search
            value={newTag}
            placeholder={t('learning.tagsNewPlaceholder')}
            onChange={setNewTag}
            onSearch={addNewTag}
          />
        </div>
        {showApplyToChildren && (
          <Checkbox checked={applyToChildren} onChange={setApplyToChildren}>
            {t('learning.tagsApplyToChildren')}
          </Checkbox>
        )}
      </div>
    </Modal>
  );
}
