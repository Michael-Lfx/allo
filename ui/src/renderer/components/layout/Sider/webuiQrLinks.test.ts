

import { describe, expect, test } from 'bun:test';
import { buildWebuiQrLoginUrl, getWebuiQrBaseUrls, listBrowserScanBaseUrls } from './webuiQrLinks';

describe('webui QR login links', () => {
  test('uses every usable advertised LAN access URL for QR login candidates', () => {
    const bases = getWebuiQrBaseUrls(
      {
        allowRemote: true,
        localUrl: 'http://localhost:25808',
        networkUrl: 'http://10.8.0.2:25808',
        networkUrls: ['http://10.8.0.2:25808', 'http://192.168.31.5:25808'],
      },
      ['http://10.8.0.2:25808', 'http://192.168.31.5:25808'],
      25808
    );

    expect(bases).toEqual(['http://10.8.0.2:25808', 'http://192.168.31.5:25808']);
    expect(bases.map((base) => buildWebuiQrLoginUrl(base, 'token value'))).toEqual([
      'http://10.8.0.2:25808/qr-login?token=token%20value',
      'http://192.168.31.5:25808/qr-login?token=token%20value',
    ]);
  });

  test('filters special-purpose IPs that mobile devices cannot use for WebUI QR login', () => {
    const bases = getWebuiQrBaseUrls(
      {
        allowRemote: true,
        localUrl: 'http://localhost:25808',
        networkUrl: 'http://198.18.0.1:25808',
        networkUrls: [
          'http://198.18.0.1:25808',
          'http://198.19.255.1:25808',
          'http://169.254.10.20:25808',
          'http://127.0.0.1:25808',
          'http://0.0.0.0:25808',
          'http://192.168.31.5:25808',
          'http://10.8.0.2:25808',
        ],
      },
      ['http://198.18.0.1:25808', 'http://192.168.31.5:25808'],
      25808
    );

    expect(bases).toEqual(['http://192.168.31.5:25808', 'http://10.8.0.2:25808']);
  });

  test('gives a phone browser only http LAN login URLs', () => {
    const bases = listBrowserScanBaseUrls(
      {
        running: true,
        allowRemote: true,
        localUrl: 'http://127.0.0.1:25808',
        networkUrls: ['http://192.168.31.5:25808', 'http://127.0.0.1:25808'],
      },
      ['http://192.168.31.5:25808', 'http://127.0.0.1:25808']
    );

    expect(bases).toEqual(['http://192.168.31.5:25808']);
    expect(buildWebuiQrLoginUrl(bases[0], 'once')).toBe('http://192.168.31.5:25808/qr-login?token=once');
  });

  test('puts a Wi-Fi address ahead of Hyper-V and WSL addresses', () => {
    expect(
      listBrowserScanBaseUrls(
        {
          running: true,
          allowRemote: true,
          networkUrls: ['http://172.30.16.1:25808', 'http://172.17.0.1:25808', 'http://192.168.2.155:25808'],
        },
        ['http://172.30.16.1:25808', 'http://172.17.0.1:25808', 'http://192.168.2.155:25808']
      )
    ).toEqual(['http://192.168.2.155:25808']);
  });

  test('keeps a virtual-adapter address only when no phone-reachable LAN address exists', () => {
    expect(
      listBrowserScanBaseUrls(
        { running: true, allowRemote: true, networkUrls: ['http://172.30.16.1:25808'] },
        ['http://172.30.16.1:25808']
      )
    ).toEqual(['http://172.30.16.1:25808']);
  });

  test('does not invent a localhost QR when the phone has no LAN address', () => {
    expect(
      listBrowserScanBaseUrls(
        {
          running: true,
          allowRemote: true,
          localUrl: 'http://localhost:25808',
          networkUrls: ['http://127.0.0.1:25808'],
        },
        []
      )
    ).toEqual([]);
    expect(listBrowserScanBaseUrls({ running: false, allowRemote: false, localUrl: 'http://localhost:25808' }, [])).toEqual(
      []
    );
  });

  test('falls back to the explicit primary LAN URL when the URL list is absent', () => {
    expect(
      getWebuiQrBaseUrls(
        {
          allowRemote: true,
          localUrl: 'http://localhost:25808',
          networkUrl: 'http://192.168.31.5:25808',
        },
        [],
        25808
      )
    ).toEqual(['http://192.168.31.5:25808']);
  });
});
