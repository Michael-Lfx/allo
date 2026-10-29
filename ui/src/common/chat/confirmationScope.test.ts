import { expect, test } from 'bun:test';
import { confirmationSubmissionScope } from './chatLib';

test('voice confirmation recovery carries the original message and exact pending registration', () => {
  const id='voice:instance:17:0190f5fe-7c00-7a00-8000-000000000001';
  expect(confirmationSubmissionScope(id,undefined,'proceed_once')).toEqual({msg_id:'0190f5fe-7c00-7a00-8000-000000000001',data:{value:'proceed_once',approval_id:id},always_allow:false});
  expect(confirmationSubmissionScope(id,undefined,'cancel').always_allow).toBe(false);
  expect(()=>confirmationSubmissionScope(id,undefined,'proceed_always')).toThrow();
  expect(()=>confirmationSubmissionScope('voice:broken',undefined,'proceed_once')).toThrow();
});

test('ordinary confirmation payloads retain their existing fields and always preference', () => {
  expect(confirmationSubmissionScope('normal-confirmation','message','proceed_always')).toEqual({msg_id:'message',data:{value:'proceed_always'},always_allow:true});
});
