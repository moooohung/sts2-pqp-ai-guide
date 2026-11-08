namespace HumanPlayRecorder;

// "내 리플레이" window copy (written by the Writer agent, 2026-11-08).
static class ReplayListText
{
    public const string Title = "내 리플레이";
    public const string Intro = "이 PC로 기록한 최근 리플레이(최대 500개)를 카드·유물 이름으로 찾을 수 있습니다.\n각 판의 공개 설정을 누르면 sts.pqp.me에 바로 적용됩니다.";
    public const string SearchPlaceholder = "카드·유물 이름 검색";
    public const string SearchEmpty = "일치하는 리플레이가 없습니다.";
    public const string SearchCount = "검색 결과 {0}개";
    public const string RowDeck = "덱 {0}장";
    public const string RowRelics = "유물 {0}개";
    public const string RowKiller = "{0}에게 패배";
    public const string RowMore = "+{0}";
    public const string RowMatched = "일치";
    public const string DeckBack = "목록으로";
    public const string DeckTitle = "{0} 최종 덱 ({1}장)";
    public const string TooltipDeck = "누르면 전체 덱을 카드로 봅니다.";
    public const string TooltipRelic = "누르면 유물 설명을 크게 봅니다.";
    public const string TooltipCard = "누르면 카드를 크게 봅니다.";
    public const string DefaultPublic = "앞으로 끝나는 판 기본 설정: 공개 목록에 올립니다.";
    public const string DefaultPrivate = "앞으로 끝나는 판 기본 설정: 나만 봅니다.";
    public const string DefaultToPrivate = "나만 보기로";
    public const string DefaultToPublic = "공개로";
    public const string OpenSite = "브라우저 열기";
    public const string Close = "닫기";
    public const string Loading = "리플레이 목록을 불러오는 중입니다...";
    public const string Empty = "아직 올라온 리플레이가 없습니다. 판이 끝나고 1~2분 뒤에 목록에 올라옵니다.";
    public const string LoadFailed = "목록을 불러오지 못했습니다. 아래 '브라우저 열기'로 직접 확인해 주세요.";
    public const string RowWin = "승리";
    public const string RowDeath = "사망";
    public const string RowAscension = "승천 {0}";
    public const string RowFloor = "{0}막 {1}층";
    public const string RowParty = "{0}인";
    public const string RowView = "보기";
    public const string RowIsPublic = "공개";
    public const string RowIsPrivate = "나만 보기";
    public const string RowSaving = "저장 중";
    public const string RowToggleFailed = "설정을 바꾸지 못했습니다. 잠시 후 다시 시도해 주세요.";
    public const string TooltipToggle = "누르면 이 리플레이의 공개·나만 보기 상태가 바로 바뀝니다.";
}
