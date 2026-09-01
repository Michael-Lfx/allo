export type ProjectStyleWorldId = "xianxia" | "urban" | "historical" | "suspense" | "science-fiction" | "pastoral" | "cyberpunk" | "republic" | "campus" | "court" | "wasteland" | "space";
export type ProjectStyleToneId = "epic" | "dark" | "light-comedy" | "romantic" | "healing" | "melancholic" | "glamour" | "documentary";
export type ProjectStyleMediumId = "live-action" | "3d-anime" | "3d-cartoon" | "2d-guoman" | "ink" | "stop-motion" | "comic" | "storybook";
export type ProjectStyleCharacterId = "realistic" | "semi-real" | "anime" | "stylized";

export type ProjectStyleSelection = {
    world: ProjectStyleWorldId;
    tone: ProjectStyleToneId;
    medium: ProjectStyleMediumId;
    character: ProjectStyleCharacterId;
};

export type CanvasStylePreset = {
    id: string;
    title: string;
    category: string;
    description: string;
    tags: string[];
    prompt: string;
    imageUrl: string;
    selection?: ProjectStyleSelection;
};

type StyleOption<T extends string> = {
    id: T;
    label: string;
    description: string;
    prompt: string;
    palette?: string;
    wardrobe?: string;
    environment?: string;
    motion?: string;
    forbidden: string;
};

export const projectStyleWorlds: Array<StyleOption<ProjectStyleWorldId>> = [
    {
        id: "xianxia",
        label: "仙侠",
        description: "宗门、仙城、洞府、云海与法术规则共同构成东方修行世界。",
        prompt: "东方仙侠世界以宗门、境界、法器、灵兽和因果秩序为基础；奇观从中式山水、木构建筑、道家意象和修行体系演化，所有地点共享明确的时代、地理与力量规则。",
        palette: "云雾白、黛青、青玉、朱砂、古铜与阵营能量色",
        wardrobe: "袍服、劲装、甲胄、宗门纹样、发冠、腰封和法器按身份与阵营建立稳定资产层级",
        environment: "山门、殿宇、楼阁、洞府、仙城、云海、古战场和凡间城镇采用中式结构与东方山水空间",
        motion: "御剑、身法、法术和衣袍运动必须有起势、蓄力、受力与落点，力量等级通过规模和环境反馈表达",
        forbidden: "欧洲城堡、哥特教堂、日式鸟居、西式魔法阵、随机游戏 UI、宗门与法术规则漂移",
    },
    {
        id: "urban",
        label: "当代都市",
        description: "以可信中国城市、职业身份与生活关系组织现代短剧。",
        prompt: "当代中国都市世界以职业、家庭、收入、社区和城市公共生活为基础；人物行动、空间功能、中文标识与生活设施必须符合明确城市和社会语境。",
        palette: "中性白、雾灰、浅木、水泥色与克制的角色识别色",
        wardrobe: "服装按职业、收入、季节和性格建立衣橱，固定配饰承担角色识别",
        environment: "办公室、公寓、学校、医院、商场、街区、社区和公共交通保持本地尺度与使用痕迹",
        motion: "表演贴近日常重心、停顿和微表情，镜头运动服务人物关系与信息揭示",
        forbidden: "欧美城市替代、空洞样板间、错误中文、无职业逻辑服装、网红滤镜和广告摆拍",
    },
    {
        id: "historical",
        label: "古装历史",
        description: "先锁定单一时代，再统一礼制、服化道、器物与建筑。",
        prompt: "历史世界必须先依据故事锁定唯一朝代或年代子类型，人物制度、礼仪、服饰、兵器、器物、交通与建筑全部服从同一考据基线。",
        palette: "米白、黛灰、木褐、土黄、靛青、竹青与有限朱砂金色",
        wardrobe: "襟形、袖型、腰带、冠帽、鞋履和纹样按时代、阶层与身份建立制度",
        environment: "木构、院落、回廊、城镇、宫署、乡野和道路遵循所选时代的结构与材料",
        motion: "人物动作考虑礼制、服装重量、兵器惯性和具体生活劳动",
        forbidden: "朝代混搭、现代拉链家具、日式或欧式建筑、塑料饰品和现代偶像妆",
    },
    {
        id: "suspense",
        label: "悬疑犯罪",
        description: "真实城市纹理、线索系统和心理压力构成可读的悬疑世界。",
        prompt: "现代悬疑世界以案件、秘密、身份关系和可追踪线索构成，地点布局、关键道具、伤痕与时间线必须稳定，信息隐藏不能牺牲画面可读性。",
        palette: "炭灰、冷水泥、旧木、脏黄实景灯与少量危险强调色",
        wardrobe: "职业服饰、制服、便装和关键随身物按身份与时间线建立版本",
        environment: "旧居民楼、办公空间、街巷、仓储、地下空间和城郊设施保持同一城市地域",
        motion: "观察、跟随、遮挡和反应镜头服务线索揭示，避免把晃动当作悬疑",
        forbidden: "黑成不可读、霓虹灯海、过量烟雾、血浆猎奇、反派脸谱化和线索资产变形",
    },
    {
        id: "science-fiction",
        label: "未来科幻",
        description: "技术规则、社会结构与功能性空间共同支撑未来叙事。",
        prompt: "未来世界必须先确定技术水平、能源、交通、通讯和社会组织规则；科技资产具有可理解的功能、接口与使用限制，奇观必须反映文明尺度而非随机堆砌发光物。",
        palette: "深空黑、冷灰、银白、舱体白与单一阵营或能源强调色",
        wardrobe: "工作服、制服、航天或防护装备按功能与阵营模块化设计",
        environment: "舰船、空间站、未来城市、实验设施和居住空间遵循结构、维护与照明逻辑",
        motion: "运动体现设备重量、惯性、空间限制和技术反馈",
        forbidden: "无功能 HUD、随机霓虹、现实品牌、技术规则漂移、装备接口变化和无尺度参照",
    },
    {
        id: "pastoral",
        label: "自然乡野",
        description: "地域、季节、生活劳动和自然环境共同参与叙事。",
        prompt: "自然乡野世界以明确地域、季节、家庭关系和生活劳动为基础；植物、天气、住宅、工具与集市不是装饰，而是持续参与人物行动和关系变化的真实环境。",
        palette: "叶绿、苔藓绿、土色、旧木、天空色、米白与季节性角色点色",
        wardrobe: "棉麻、针织、旧牛仔、工作服、雨具和手作物保留褶皱与使用痕迹",
        environment: "村镇住宅、林间小屋、农场、河岸、菜园、集市和小城边缘具有地域证据",
        motion: "风、雨、植物、动物和细小生活动作形成自然节奏",
        forbidden: "商业民宿广告感、欧美乡村替代、随机换季、无生活痕迹和过度航拍空镜",
    },
    {
        id: "cyberpunk",
        label: "赛博都市",
        description: "潮湿夜城、阶层义体与受控霓虹构成技术失衡的近未来。",
        prompt: "赛博都市世界以亚洲高密度夜城、企业领地、地下黑市和义体阶层为基础；霓虹、雨水、旧材质与界面光必须服务功能与身份，而不是把整座城市涂成装饰灯牌。",
        palette: "湿沥青、冷青、品红霓虹、锈橙指示灯与少量企业识别色",
        wardrobe: "机能外套、义体接口、阶层制服和街头改装按身份模块化，保留磨损与改装痕迹",
        environment: "高楼峡谷、天桥、雨巷、地下商场、维修巷道和企业大堂共享同一城市地理",
        motion: "人群、交通、雨水和屏幕光斑形成城市节奏，人物动作考虑装备重量与狭窄空间",
        forbidden: "无功能 HUD 满屏、纯欧美赛博模板、霓虹淹没人物、干燥白日棚拍和随机游戏 UI",
    },
    {
        id: "republic",
        label: "港风年代",
        description: "八九十年代华语都市的胶片色、街区密度与生活器物。",
        prompt: "港风年代世界锁定二十世纪后期华语都市：霓虹招牌、骑楼、茶餐厅、码头、老公寓与夜生活共同构成时代地理；色彩来自胶片、钨丝灯和潮湿空气，而不是复古滤镜贴纸。",
        palette: "钨丝暖黄、青绿阴影、褪色品红招牌与胶片中性肤色",
        wardrobe: "衬衫、风衣、针织、旗袍剪裁与皮鞋按阶层和职业建立衣橱，避免当代潮牌混入",
        environment: "骑楼街道、茶餐厅、旧公寓、码头、报摊、舞厅和雨夜马路保持同一座城市的尺度",
        motion: "手持或平稳跟拍贴近生活重心，夜景光源来自招牌、车灯与室内钨丝",
        forbidden: "当代手机界面、错误繁简混用招牌、欧美复古替代、过度颗粒滤镜和时代器物漂移",
    },
    {
        id: "campus",
        label: "校园青春",
        description: "学期节奏、班级关系和校园空间组织青春短剧。",
        prompt: "校园青春世界以学期、班级、社团和放学路径组织人物关系；教室、操场、走廊、宿舍和校门口必须像被使用过的真实校园，而不是偶像剧布景。",
        palette: "校服识别色、课桌木色、操场绿、黄昏暖金与晴空浅蓝",
        wardrobe: "校服、运动鞋、季节外套和书包配饰按年级与性格建立差异，保持可复用资产",
        environment: "教学楼、操场、天台、便利店、公交站和回家路线构成稳定校园地理",
        motion: "课间、奔跑、停顿与对视构成青春节奏，避免无动机的慢动作花瓣",
        forbidden: "成人职场空间替代校园、过度磨皮网红脸、永恒樱花滤镜和校服制度漂移",
    },
    {
        id: "court",
        label: "宫廷权谋",
        description: "礼制、仪仗与权力空间共同支撑宫廷叙事。",
        prompt: "宫廷权谋世界以单一王朝礼制为核心：朝会、内廷、花园、寝殿和仪仗队列必须服从身份差序；华丽来自织物、建筑等级和仪式，而不是堆金砌玉的游戏UI。",
        palette: "朱红、黛青、冷金、牙白、深褐木作与有限夜灯暖色",
        wardrobe: "冕服、常服、甲胄和宫人衣装按品级、场合建立制度，纹样不得跨级混用",
        environment: "宫殿轴线、廊庑、御道、内苑和水榭遵循同一都城规划",
        motion: "步伐、跪拜、仪仗和衣袂重量体现礼制，权力对峙用距离与停顿表达",
        forbidden: "朝代混搭、日式或欧式宫殿、塑料金饰、现代妆容和礼制等级漂移",
    },
    {
        id: "wasteland",
        label: "末日废土",
        description: "资源稀缺、废墟地理与改装生存装备构成末日世界。",
        prompt: "末日废土世界以资源、气候伤害和残存聚落为基础；废墟必须能读出灾前功能，装备来自改装与回收，奇观服务于生存压力而不是无来源爆炸。",
        palette: "尘土、锈橙、骨白、煤灰与稀少的警示色或旧世界残彩",
        wardrobe: "防护层、缝补织物、护目镜和改装包具按聚落与职能区分",
        environment: "坍塌城市、公路、掩体、集市残骸和干旱地貌保持同一灾后地理",
        motion: "动作考虑负重、沙尘、掩体和体力限制，镜头避免无动机的毁灭奇观",
        forbidden: "崭新科幻盔甲、绿洲广告感、无磨损道具、随机核爆闪光和地理漂移",
    },
    {
        id: "space",
        label: "星际远征",
        description: "舰船结构、阵营徽记与真空尺度支撑太空叙事。",
        prompt: "星际远征世界必须先确定推进、重力、通讯和阵营政治；舰船、空间站与行星地表具有可理解的结构和维护痕迹，宏大来自尺度对照而不是乱堆星云。",
        palette: "深空黑、舱体白、冷金属、观察窗蓝与阵营徽记色",
        wardrobe: "舰员服、防护舱外装和典礼服按职能模块化，徽记与接口保持稳定",
        environment: "舰桥、走廊、船坞、行星前哨和观察窗对外空间共享同一技术文明",
        motion: "失重或人工重力下的惯性、舱门气压和设备反馈必须可见",
        forbidden: "无结构星云背景、现实品牌、随机激光秀、技术规则漂移和无尺度参照",
    },
];

export const projectStyleTones: Array<StyleOption<ProjectStyleToneId>> = [
    {
        id: "epic",
        label: "宏大叙事",
        description: "用尺度、秩序与人物命运建立史诗感，不靠全程阴暗和特效堆积。",
        prompt: "叙事气质庄重、开阔、有历史与命运重量；宏大场面负责建立空间和力量关系，人物镜头负责承接选择与代价，奇观和表演之间保持清晰主次。",
        palette: "高明度空间层次、稳定阵营色与少量高亮奇观色，暗部保留细节",
        motion: "建立镜头克制而有尺度，高潮运动有明确起止，人物反应获得停顿",
        forbidden: "全片低照度、无休止慢动作、每镜航拍、特效盖住人物、只有规模没有情绪落点",
    },
    {
        id: "dark",
        label: "暗黑诡谲",
        description: "以未知、禁忌和心理压力形成黑暗气质，同时保持信息可读。",
        prompt: "叙事气质危险、克制、带未知感；恐惧来自空间、规则和人物反应，不依赖血浆、全黑画面或持续尖叫。",
        palette: "低调但可读的冷暖分区，危险色只用于禁忌、敌意或线索",
        motion: "更多观察、遮挡、缓慢揭示和反应停顿，爆发运动只用于关键破局",
        forbidden: "黑成不可读、血腥猎奇、廉价红黑滤镜、全屏烟雾、无来源闪光和持续镜头抖动",
    },
    {
        id: "light-comedy",
        label: "轻喜剧",
        description: "宏观处境与人物一本正经的反应形成反差，画面明快但不幼稚。",
        prompt: "叙事气质轻松、机敏、有人情味；笑点依靠人物关系、反应、停顿和处境反差，允许宏大世界承载小人物式幽默，不把喜剧等同于夸张鬼脸。",
        palette: "清晰明快的中高明度色彩，角色与背景保持稳定分离，危险场面也避免恐怖化压黑",
        motion: "动作有清楚预备、落点和喜剧停顿，反应镜头与正经表演优先于炫技运镜",
        forbidden: "阴森恐怖基调、儿童节目式糖果色、所有角色做鬼脸、连续快切和笑点被特效淹没",
    },
    {
        id: "romantic",
        label: "唯美浪漫",
        description: "以人物距离、光线和环境细节表达关系变化，避免滤镜化糖水感。",
        prompt: "叙事气质细腻、含蓄、具有关系张力；浪漫来自视线、距离、触碰、环境变化和未说出口的情绪，不依赖随机花瓣与持续柔焦。",
        palette: "自然肤色、柔和冷暖关系与克制的情绪点色",
        motion: "镜头靠近人物和细节，运动缓慢而有情绪动机，保留呼吸与停顿",
        forbidden: "全程柔焦、过曝肤色、随机花瓣、MV 式空镜、无表演的慢动作和滤镜漂移",
    },
    {
        id: "healing",
        label: "温暖治愈",
        description: "通过生活行动、环境触感和关系修复形成温暖，而不是泛化柔光。",
        prompt: "叙事气质温暖、松弛、具体；治愈来自可见的照料、劳动、陪伴和关系变化，环境材质与自然声音参与情绪表达。",
        palette: "自然中高明度、温暖但不过黄，保留木、布、植物和肤色的真实层次",
        motion: "观察细小动作、呼吸、眼神和环境响应，节奏舒展但不空洞",
        forbidden: "全片奶油滤镜、过度暖黄、无剧情自然空镜、广告式精致和人物永远微笑",
    },
    {
        id: "melancholic",
        label: "清冷克制",
        description: "低饱和、留白和克制表演形成文艺气质，信息仍然可读。",
        prompt: "叙事气质清冷、克制、带距离感；情绪来自停顿、天气、室内灯色和未说完的对白，色彩降低饱和但保留肤色与关键道具的可识别性。",
        palette: "灰蓝、冷白、浅木与少量温暖室内灯，暗部保留层次",
        motion: "镜头平稳、剪辑留白，运动只服务关系变化，避免炫技摇移",
        forbidden: "全片发灰不可读、过度青橙、无表演空镜和把清冷做成恐怖片",
    },
    {
        id: "glamour",
        label: "华丽时尚",
        description: "服化道、灯光造型和构图共同建立时尚大片气质。",
        prompt: "叙事气质精致、自信、有造型意识；华丽来自剪裁、珠宝、布光和姿态，而不是磨皮美颜或乱闪的闪光灯。",
        palette: "深底色、金属高光、珠宝点色与受控的皮肤高光",
        motion: "姿态明确、走位如造型，镜头可用缓慢推拉但必须有构图意图",
        forbidden: "网红滤镜、塑料皮肤、过曝闪光、廉价金粉和服饰品牌乱入",
    },
    {
        id: "documentary",
        label: "纪实观察",
        description: "观察式机位、自然光和生活痕迹构成纪实基线。",
        prompt: "叙事气质诚实、贴近、不打断生活；摄影机像在场的观察者，光来自环境，人物不必看镜头，空间保留使用痕迹。",
        palette: "现场光源色温、自然灰土与未调高的环境色",
        motion: "跟拍、等待、轻微手持，避免广告式编排走位",
        forbidden: "棚拍光、摆拍笑容、过饱和旅拍滤镜和把纪实做成监控模糊",
    },
];

export const projectStyleMedia: Array<StyleOption<ProjectStyleMediumId> & { characters: ProjectStyleCharacterId[] }> = [
    {
        id: "live-action",
        label: "真人实拍",
        description: "真实演员、可信光学、服化道和物理环境。",
        prompt: "采用真人影视拍摄媒介：真实人物比例、自然皮肤与发丝、可信镜头光学和现场光源，服装、建筑、道具与特效保持真实材质响应。",
        motion: "动作遵循真实重心、惯性、衣料重量和摄影机物理运动",
        forbidden: "动画角色、游戏 CG、塑料皮肤、过度磨皮、虚假棚拍和无物理依据的材质",
        characters: ["realistic", "semi-real"],
    },
    {
        id: "3d-anime",
        label: "3D 动漫",
        description: "高品质三维动画、东方动漫造型与电影化场景。",
        prompt: "采用高品质 3D 动漫媒介：清晰三维体积、统一 PBR 或风格化 PBR 材质、受控次表面散射、稳定拓扑和电影级灯光；保持动画角色的设计感，不滑向真人恐怖谷或廉价游戏过场。",
        motion: "动画具备明确关键姿势、预备动作、跟随、受力和停顿，特效与角色动作共享同一空间和光照",
        forbidden: "廉价手游 CG、塑料手办反光、僵硬待机动作、真人毛孔皮肤、角色与背景渲染体系不一致",
        characters: ["semi-real", "anime"],
    },
    {
        id: "3d-cartoon",
        label: "3D 卡通",
        description: "风格化比例、清晰剪影、柔和材质和可读表情。",
        prompt: "采用风格化 3D 卡通动画媒介：简练几何、清晰剪影、半哑光材质、柔和次表面散射和可读表情；场景与角色使用同一圆角、粗糙度和细节密度体系。",
        motion: "角色运动使用适度挤压拉伸、跟随、缓入缓出和清晰停顿，夸张建立在稳定骨架与关节结构上",
        forbidden: "塑料公仔、所有角色同脸、过大玻璃眼、僵硬 T Pose、关节穿模和写实资产混入",
        characters: ["stylized"],
    },
    {
        id: "2d-guoman",
        label: "国漫 2D",
        description: "稳定线稿、赛璐璐角色与东方绘景体系。",
        prompt: "采用国漫 2D 动画媒介：有粗细变化的稳定线稿、两至三阶赛璐璐明暗、清晰角色剪影和东方绘景层次；全项目保持二维绘画语言。",
        motion: "使用明确关键姿势、有限但准确的二维运动，口型、发丝、衣摆与特效遵循统一帧间节奏",
        forbidden: "三维塑料质感、真人照片、线宽漂移、角色转面失真、背景与角色画法不一致",
        characters: ["semi-real", "anime", "stylized"],
    },
    {
        id: "ink",
        label: "水墨动画",
        description: "宣纸、墨阶、笔势与留白构成可识别的东方叙事。",
        prompt: "采用中国水墨动画媒介：宣纸纤维、干湿笔触、飞白、积墨、破墨与有限工笔勾线；留白、墨阶和固定点色共同塑造空间与人物身份。",
        motion: "轮廓先行、笔势跟随、墨迹生长与消散遵循统一规则，人物结构在变化中保持可识别",
        forbidden: "随机泼墨、角色五官消失、西式水彩、全屏脏灰、廉价纸纹滤镜和随机文字印章",
        characters: ["semi-real", "stylized"],
    },
    {
        id: "stop-motion",
        label: "定格黏土",
        description: "手塑痕迹、微缩布景与逐帧动作构成可触的手工世界。",
        prompt: "采用定格黏土或偶戏媒介：保留指纹与接缝的手塑形体、微缩场景、摄影棚柔光和轻微逐帧顿挫；材质来自黏土、织物、纸木，而不是光滑 CG 公仔。",
        motion: "动作呈可感知的帧步进，预备、停顿和跟随被放大，避免流畅到像三维实时渲染",
        forbidden: "无接缝塑料 CG、真人皮肤、流畅动作捕捉感和比例随机漂移",
        characters: ["stylized"],
    },
    {
        id: "comic",
        label: "漫画分镜",
        description: "墨线、网点与高对比块面构成可读的漫画影像。",
        prompt: "采用漫画/图形小说媒介：稳定墨线、网点或平涂色块、戏剧性分格光影；夸张来自构图、速度线和姿态，角色剪影在转面中保持连续。",
        motion: "关键姿势清晰、帧间跳跃服务节奏，拟声与速度线不得淹没身份特征",
        forbidden: "照片滤镜漫画、线宽随机漂移、所有角色同脸和把漫画做成低幼简笔画",
        characters: ["stylized", "anime"],
    },
    {
        id: "storybook",
        label: "绘本插画",
        description: "纸张颗粒、柔和色块与手绘边缘构成童话绘本世界。",
        prompt: "采用绘本插画媒介：可见纸纹或颜料边缘、柔和色块、手绘轮廓；角色表情清楚，背景层次服务故事，不把绘本感做成全屏柔焦滤镜。",
        motion: "动作温和、转场如翻页或色块推移，保持二维绘画语言",
        forbidden: "三维塑料质感、真人照片、过锐数字描边和随机贴纸装饰",
        characters: ["stylized", "semi-real"],
    },
];

export const projectStyleCharacters: Array<StyleOption<ProjectStyleCharacterId>> = [
    {
        id: "realistic",
        label: "写实人物",
        description: "真实骨相、年龄、皮肤和身体比例。",
        prompt: "角色采用真实东亚骨相、自然年龄和真实身体比例，通过脸型、五官、发型、体型、姿态与生活痕迹建立差异；保留自然皮肤和微表情。",
        forbidden: "网红同脸、过度磨皮、幼态大眼、年龄漂移、欧美默认脸和夸张动漫比例",
    },
    {
        id: "semi-real",
        label: "半写实仿真人",
        description: "东方骨相与理想化造型平衡，保留可信结构。",
        prompt: "角色采用半写实东方造型：可信骨骼、肌肉和关节结构，五官适度理想化但保留年龄与身份差异；发丝、皮肤和服装细节服从所选媒介，不进入恐怖谷。",
        forbidden: "真人扫描僵硬感、统一偶像脸、毛孔级过度写实、假睫毛模板和身体比例漂移",
    },
    {
        id: "anime",
        label: "动漫角色",
        description: "修长比例、清晰五官设计和稳定角色剪影。",
        prompt: "角色采用东方动漫造型：约 7 至 8 头身，五官、发束、眼形和服装结构高度可识别；夸张集中在设计语言和表情，不破坏身体结构与身份稳定性。",
        forbidden: "低幼大头、过大玻璃眼、随机发色、所有角色同一脸型、五官在转面中漂移",
    },
    {
        id: "stylized",
        label: "风格化卡通",
        description: "夸张比例与可读表情服务喜剧和轻松叙事。",
        prompt: "角色采用统一的风格化卡通比例：成人约 5 至 6 头身，儿童约 3.5 至 4.5 头身，头手可适度夸张；通过眼鼻嘴比例、发型、体型和剪影建立显著差异。",
        forbidden: "所有角色同脸、头身随机变化、过大玻璃眼、关节结构错误、塑料玩具感和僵硬表情",
    },
];

export const defaultProjectStyleSelection: ProjectStyleSelection = {
    world: "xianxia",
    tone: "epic",
    medium: "3d-anime",
    character: "semi-real",
};

const STYLE_SCOPE = "【使用边界】本规范锁定项目级题材世界、叙事气质、视觉媒介、角色造型、资产材质与影像基线。单镜头的剧情动作、构图、景别、机位、运镜、光源位置和天气由分镜按本规范创作，不得机械复制项目级描述。";

export function compatibleProjectStyleCharacters(mediumId: ProjectStyleMediumId) {
    const allowed = projectStyleMedia.find((item) => item.id === mediumId)?.characters || [];
    return projectStyleCharacters.filter((item) => allowed.includes(item.id));
}

export function compileCanvasStylePreset(selection: ProjectStyleSelection): CanvasStylePreset {
    const world = requiredOption(projectStyleWorlds, selection.world);
    const tone = requiredOption(projectStyleTones, selection.tone);
    const medium = requiredOption(projectStyleMedia, selection.medium);
    const compatibleCharacters = compatibleProjectStyleCharacters(selection.medium);
    const character = compatibleCharacters.find((item) => item.id === selection.character) || compatibleCharacters[0];
    const resolvedSelection = { ...selection, character: character.id };
    const title = `${world.label} · ${tone.label} · ${medium.label}`;
    return {
        id: styleSelectionId(resolvedSelection),
        title,
        category: `${world.label} / ${medium.label}`,
        description: `${world.description}${tone.description}${medium.description}`,
        tags: [world.label, tone.label, medium.label, character.label],
        imageUrl: stylePreviewImage(resolvedSelection),
        selection: resolvedSelection,
        prompt: [
            `【风格组合】题材世界：${world.label}；叙事气质：${tone.label}；视觉媒介：${medium.label}；角色造型：${character.label}。以下四项必须同时成立，后续角色、场景、分镜和视频不得擅自替换其中任一维度。`,
            `【项目定位】面向“${world.label}”题材的${tone.label}${medium.label}项目，以统一世界规则、角色资产和视觉媒介支撑连续叙事；项目规范只定义长期稳定的美术基线，不代替单镜头导演设计。`,
            `【题材世界观】${world.prompt}`,
            `【叙事气质】${tone.prompt}`,
            `【视觉媒介】${medium.prompt} 成片保持高清可读的主体轮廓、稳定材质和受控高光；高清不等于过度锐化、磨皮或无来源泛光。`,
            `【角色设计系统】${character.prompt}`,
            `【项目色彩与光影】基础色彩来自${world.palette}；本项目采用${tone.palette}。色彩按角色、阵营、地点和叙事功能分配，保证人物与环境具有稳定的明度和色相分离。`,
            `【服饰、材质与场景】${world.wardrobe}；${world.environment}。所有材质和细节密度遵循“${medium.label}”媒介，不混入其他渲染体系。`,
            `【影像与动态基线】${tone.motion}；${medium.motion}；${world.motion}。分镜必须先确定镜头叙事意图，再选择景别与运镜。`,
            "【资产一致性】角色固定脸型、五官比例、发型、头身、服装、道具和阵营色；主要场景固定空间布局、建筑模块、材质与照明基线；换装、受损、升级和时间变化必须形成明确资产版本。",
            `【全局禁用】${world.forbidden}；${tone.forbidden}；${medium.forbidden}；${character.forbidden}。`,
            STYLE_SCOPE,
        ].join("\n"),
    };
}

export function parseCanvasStyleSelection(id?: string): ProjectStyleSelection | null {
    if (!id?.startsWith("v2-")) return null;
    const parts = id.slice(3).split("--");
    if (parts.length !== 4) return null;
    const selection = { world: parts[0], tone: parts[1], medium: parts[2], character: parts[3] } as ProjectStyleSelection;
    if (!projectStyleWorlds.some((item) => item.id === selection.world)
        || !projectStyleTones.some((item) => item.id === selection.tone)
        || !projectStyleMedia.some((item) => item.id === selection.medium)
        || !projectStyleCharacters.some((item) => item.id === selection.character)) return null;
    return selection;
}

export function customCanvasStylePreset(id?: string) {
    const selection = parseCanvasStyleSelection(id);
    return selection ? compileCanvasStylePreset(selection) : undefined;
}

const recommendedSelections: ProjectStyleSelection[] = [
    { world: "xianxia", tone: "epic", medium: "3d-anime", character: "semi-real" },
    { world: "xianxia", tone: "dark", medium: "3d-anime", character: "semi-real" },
    { world: "xianxia", tone: "light-comedy", medium: "3d-cartoon", character: "stylized" },
    { world: "xianxia", tone: "epic", medium: "live-action", character: "realistic" },
    { world: "xianxia", tone: "healing", medium: "ink", character: "semi-real" },
    { world: "urban", tone: "light-comedy", medium: "live-action", character: "realistic" },
    { world: "urban", tone: "melancholic", medium: "live-action", character: "realistic" },
    { world: "urban", tone: "glamour", medium: "live-action", character: "realistic" },
    { world: "urban", tone: "documentary", medium: "live-action", character: "realistic" },
    { world: "campus", tone: "romantic", medium: "live-action", character: "realistic" },
    { world: "suspense", tone: "dark", medium: "live-action", character: "realistic" },
    { world: "historical", tone: "romantic", medium: "2d-guoman", character: "semi-real" },
    { world: "court", tone: "epic", medium: "live-action", character: "realistic" },
    { world: "republic", tone: "melancholic", medium: "live-action", character: "realistic" },
    { world: "cyberpunk", tone: "dark", medium: "live-action", character: "realistic" },
    { world: "science-fiction", tone: "epic", medium: "live-action", character: "realistic" },
    { world: "space", tone: "epic", medium: "3d-anime", character: "semi-real" },
    { world: "wasteland", tone: "dark", medium: "live-action", character: "realistic" },
    { world: "pastoral", tone: "healing", medium: "3d-cartoon", character: "stylized" },
    { world: "pastoral", tone: "healing", medium: "storybook", character: "stylized" },
    { world: "urban", tone: "light-comedy", medium: "comic", character: "stylized" },
    { world: "urban", tone: "light-comedy", medium: "stop-motion", character: "stylized" },
];

export const recommendedCanvasStylePresets = recommendedSelections.map(compileCanvasStylePreset);

function styleSelectionId(selection: ProjectStyleSelection) {
    return `v2-${selection.world}--${selection.tone}--${selection.medium}--${selection.character}`;
}

/** Resolve style preview assets under Vite `public/short-drama-styles`. */
export function canvasStyleAssetUrl(fileName: string) {
    const base = (import.meta.env.BASE_URL || "/").replace(/\/?$/, "/");
    const name = fileName.replace(/^\/?(?:short-drama-styles\/)?/, "");
    return `${base}short-drama-styles/${name}`;
}

function stylePreviewImage(selection: ProjectStyleSelection) {
    if (selection.medium === "stop-motion") return canvasStyleAssetUrl("clay-stop-motion.jpg");
    if (selection.medium === "comic") return canvasStyleAssetUrl("comic-pop.jpg");
    if (selection.medium === "storybook") return canvasStyleAssetUrl("storybook-fantasy.jpg");
    if (selection.world === "cyberpunk") return canvasStyleAssetUrl("cyberpunk-neon.jpg");
    if (selection.world === "republic") return canvasStyleAssetUrl("retro-hong-kong.jpg");
    if (selection.world === "campus") return canvasStyleAssetUrl("urban-live-action.jpg");
    if (selection.world === "court") return canvasStyleAssetUrl("period-live-action.jpg");
    if (selection.world === "wasteland") return canvasStyleAssetUrl("suspense-noir.jpg");
    if (selection.world === "space") return canvasStyleAssetUrl("space-opera.jpg");
    if (selection.world === "xianxia") {
        if (selection.medium === "live-action") return canvasStyleAssetUrl("period-live-action.jpg");
        if (selection.medium === "3d-cartoon") return canvasStyleAssetUrl("three-d-cartoon.jpg");
        if (selection.medium === "2d-guoman") return canvasStyleAssetUrl("chinese-2d.jpg");
        if (selection.medium === "ink") return canvasStyleAssetUrl("ink-narrative.jpg");
        return canvasStyleAssetUrl("fantasy-3d.jpg");
    }
    if (selection.world === "suspense") return canvasStyleAssetUrl("suspense-noir.jpg");
    if (selection.world === "science-fiction") return canvasStyleAssetUrl("future-tech.jpg");
    if (selection.world === "pastoral") return canvasStyleAssetUrl("nature-healing.jpg");
    if (selection.world === "historical") return selection.medium === "2d-guoman" ? canvasStyleAssetUrl("chinese-2d.jpg") : canvasStyleAssetUrl("period-live-action.jpg");
    return selection.medium === "3d-cartoon" ? canvasStyleAssetUrl("three-d-cartoon.jpg") : canvasStyleAssetUrl("urban-live-action.jpg");
}

function requiredOption<T extends string>(options: Array<StyleOption<T>>, id: T) {
    const option = options.find((item) => item.id === id);
    if (!option) throw new Error(`未知项目画风选项：${id}`);
    return option;
}
