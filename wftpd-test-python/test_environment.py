#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
快速测试脚本 - 验证Python测试环境
"""

import sys
import os

def test_imports():
    """测试必要的库导入"""
    print("测试库导入...")
    
    try:
        import ftplib
        print("✓ ftplib 导入成功")
    except ImportError as e:
        print(f"✗ ftplib 导入失败: {e}")
        return False
    
    try:
        import paramiko
        print("✓ paramiko 导入成功")
    except ImportError as e:
        print(f"✗ paramiko 导入失败: {e}")
        return False
    
    try:
        import json
        print("✓ json 导入成功")
    except ImportError as e:
        print(f"✗ json 导入失败: {e}")
        return False
    
    return True

def test_config():
    """测试配置文件"""
    print("\n测试配置文件...")
    
    if os.path.exists("test_config.json"):
        print("✓ 配置文件存在")
        
        try:
            import json
            with open("test_config.json", 'r', encoding='utf-8') as f:
                config = json.load(f)
            print("✓ 配置文件格式正确")
            
            # 检查必要字段
            required_sections = ["server", "user", "test_settings"]
            for section in required_sections:
                if section in config:
                    print(f"✓ 配置段 '{section}' 存在")
                else:
                    print(f"✗ 配置段 '{section}' 缺失")
                    return False
                    
            return True
            
        except Exception as e:
            print(f"✗ 配置文件解析失败: {e}")
            return False
    else:
        print("✗ 配置文件不存在")
        return False

def test_directory_structure():
    """测试目录结构"""
    print("\n测试目录结构...")
    
    required_dirs = ["testdata"]
    required_files = ["wftpd_test.py", "test_config.json", "requirements.txt"]
    
    for dir_name in required_dirs:
        if os.path.exists(dir_name):
            print(f"✓ 目录 '{dir_name}' 存在")
        else:
            print(f"✗ 目录 '{dir_name}' 不存在")
            # 创建缺失的目录
            try:
                os.makedirs(dir_name, exist_ok=True)
                print(f"✓ 已创建目录 '{dir_name}'")
            except Exception as e:
                print(f"✗ 创建目录失败: {e}")
                return False
    
    for file_name in required_files:
        if os.path.exists(file_name):
            print(f"✓ 文件 '{file_name}' 存在")
        else:
            print(f"✗ 文件 '{file_name}' 不存在")
    
    return True

def main():
    """主测试函数"""
    print("=" * 40)
    print("WFTPD Python 测试环境验证")
    print("=" * 40)
    
    tests = [
        ("库导入测试", test_imports),
        ("配置文件测试", test_config),
        ("目录结构测试", test_directory_structure)
    ]
    
    all_passed = True
    
    for test_name, test_func in tests:
        print(f"\n执行{test_name}...")
        if test_func():
            print(f"✓ {test_name} 通过")
        else:
            print(f"✗ {test_name} 失败")
            all_passed = False
    
    print("\n" + "=" * 40)
    if all_passed:
        print("✓ 所有环境测试通过！")
        print("可以运行: python wftpd_test.py")
    else:
        print("✗ 存在环境配置问题")
        print("请检查上述错误信息")
    
    print("=" * 40)
    
    return all_passed

if __name__ == "__main__":
    success = main()
    sys.exit(0 if success else 1)