# -*- coding: utf-8 -*-
"""
WFTPG 日志文件识别测试脚本
用于验证前端 GUI 能否正确识别和读取日志文件
"""

import os
import json
from datetime import datetime
from pathlib import Path


def test_log_file_matching():
    """测试日志文件匹配逻辑"""
    
    log_dir = r"C:\ProgramData\wftpg\logs"
    
    if not os.path.exists(log_dir):
        print(f"日志目录不存在：{log_dir}")
        return
    
    print("="*60)
    print("WFTPG 日志文件识别测试")
    print("="*60)
    print(f"\n日志目录：{log_dir}\n")
    
    # 列出所有日志文件
    all_files = os.listdir(log_dir)
    
    wftpg_logs = []
    file_ops_logs = []
    
    for filename in all_files:
        if filename.endswith('.log'):
            # 测试新的匹配逻辑
            if filename.startswith('wftpg.') or filename.startswith('wftpg-'):
                wftpg_logs.append(filename)
            elif filename.startswith('file-ops.') or filename.startswith('file-ops-'):
                file_ops_logs.append(filename)
    
    print(f"找到 {len(wftpg_logs)} 个系统日志文件:")
    for log in sorted(wftpg_logs):
        full_path = os.path.join(log_dir, log)
        mtime = os.path.getmtime(full_path)
        mtime_str = datetime.fromtimestamp(mtime).strftime('%Y-%m-%d %H:%M:%S')
        size = os.path.getsize(full_path)
        print(f"  - {log:30} (修改时间：{mtime_str}, 大小：{size:>10} bytes)")
    
    print(f"\n找到 {len(file_ops_logs)} 个文件操作日志文件:")
    for log in sorted(file_ops_logs):
        full_path = os.path.join(log_dir, log)
        mtime = os.path.getmtime(full_path)
        mtime_str = datetime.fromtimestamp(mtime).strftime('%Y-%m-%d %H:%M:%S')
        size = os.path.getsize(full_path)
        print(f"  - {log:30} (修改时间：{mtime_str}, 大小：{size:>10} bytes)")
    
    # 按修改时间排序
    print("\n" + "="*60)
    print("按修改时间排序（最新的在前）")
    print("="*60)
    
    all_logs = [(f, os.path.getmtime(os.path.join(log_dir, f))) 
                for f in all_files if f.endswith('.log')]
    all_logs.sort(key=lambda x: x[1], reverse=True)
    
    for i, (filename, mtime) in enumerate(all_logs[:10], 1):  # 只显示最新 10 个
        mtime_str = datetime.fromtimestamp(mtime).strftime('%Y-%m-%d %H:%M:%S')
        full_path = os.path.join(log_dir, filename)
        size = os.path.getsize(full_path)
        
        # 判断类型
        log_type = "系统日志" if (filename.startswith('wftpg.') or filename.startswith('wftpg-')) else \
                   "文件日志" if (filename.startswith('file-ops.') or filename.startswith('file-ops-')) else \
                   "其他"
        
        print(f"{i:2}. {filename:35} [{log_type}] (修改时间：{mtime_str}, 大小：{size:>10} bytes)")
    
    # 读取最新文件的最后几行
    if all_logs:
        latest_log = all_logs[0][0]
        print(f"\n{'='*60}")
        print(f"读取最新日志文件：{latest_log}")
        print("="*60)
        
        full_path = os.path.join(log_dir, latest_log)
        try:
            with open(full_path, 'r', encoding='utf-8') as f:
                lines = f.readlines()
                print(f"\n文件总行数：{len(lines)}")
                print(f"\n最后 5 行内容:")
                print("-"*60)
                for line in lines[-5:]:
                    try:
                        data = json.loads(line.strip())
                        timestamp = data.get('timestamp', 'N/A')
                        level = data.get('level', 'N/A')
                        message = data.get('fields', {}).get('message', 'N/A')
                        print(f"[{level}] {message[:80]}")
                    except:
                        print(line.strip()[:80])
        except Exception as e:
            print(f"读取失败：{e}")
    
    print(f"\n{'='*60}")
    print("测试完成")
    print("="*60)


if __name__ == '__main__':
    test_log_file_matching()
